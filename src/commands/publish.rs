use super::{OutputFormat, eprint_report, print_json, resolve_tag};
use livreur::checksum::sha256sums;
use livreur::{
    Config, Diagnostic, DiagnosticReport, Forge, ReleaseArtifact, ReleaseView, ResolvedConfig,
    default_forge, render_release_notes, sha256_hex, validate_release_template,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
struct PublishedAsset {
    name: String,
    sha256: String,
}

#[derive(Serialize)]
struct PublishJsonOutput {
    ok: bool,
    tag: Option<String>,
    published: bool,
    assets: Vec<PublishedAsset>,
    checksums: Option<&'static str>,
    warnings: Vec<String>,
    errors: Vec<Diagnostic>,
}

pub fn publish(
    config: &Path,
    manifest: &Path,
    tag_flag: Option<&str>,
    format: OutputFormat,
) -> i32 {
    match try_publish(config, manifest, tag_flag) {
        Ok(success) => {
            match format {
                OutputFormat::Human if success.published => println!(
                    "published {}: {} assets + SHA256SUMS",
                    success.tag,
                    success.assets.len()
                ),
                OutputFormat::Human => println!("{} is already published", success.tag),
                OutputFormat::Json => print_json(&PublishJsonOutput {
                    ok: true,
                    tag: Some(success.tag),
                    published: success.published,
                    assets: success.assets,
                    checksums: success.published.then_some("SHA256SUMS"),
                    warnings: vec![],
                    errors: vec![],
                }),
            }
            0
        }
        Err((tag, report)) => {
            match format {
                OutputFormat::Human => eprint_report(&report),
                OutputFormat::Json => print_json(&PublishJsonOutput {
                    ok: false,
                    tag,
                    published: false,
                    assets: vec![],
                    checksums: None,
                    warnings: vec![],
                    errors: report.errors,
                }),
            }
            2
        }
    }
}

struct PublishSuccess {
    tag: String,
    published: bool,
    assets: Vec<PublishedAsset>,
}

fn try_publish(
    config: &Path,
    manifest: &Path,
    tag_flag: Option<&str>,
) -> Result<PublishSuccess, (Option<String>, DiagnosticReport)> {
    let resolved = Config::load(config, manifest).map_err(|report| (None, report))?;
    validate_release_template(&resolved).map_err(|report| (None, report))?;
    let tag = resolve_tag(tag_flag).map_err(|report| (None, report))?;
    resolved
        .for_tag(&tag)
        .map_err(|report| (Some(tag.clone()), report))?;
    let forge = default_forge();
    let release = forge
        .view_release(&tag)
        .map_err(|report| (Some(tag.clone()), report))?
        .ok_or_else(|| {
            (
                Some(tag.clone()),
                DiagnosticReport::one(
                    "release",
                    format!("no release for tag {tag}; run `livreur build` first"),
                ),
            )
        })?;
    let expected = resolved.expected_assets(&tag);
    verify_expected_assets(&resolved, &release, &expected)
        .map_err(|report| (Some(tag.clone()), report))?;
    if !release.is_draft {
        return Ok(PublishSuccess {
            tag,
            published: false,
            assets: vec![],
        });
    }

    let assets = publish_draft(&resolved, forge.as_ref(), &tag, &expected)
        .map_err(|report| (Some(tag.clone()), report))?;
    Ok(PublishSuccess {
        tag,
        published: true,
        assets,
    })
}

fn verify_expected_assets(
    resolved: &ResolvedConfig,
    release: &ReleaseView,
    expected: &[String],
) -> Result<(), DiagnosticReport> {
    let mut missing = DiagnosticReport { errors: vec![] };
    for (target, asset) in resolved.targets.iter().zip(expected) {
        if !release.assets.iter().any(|current| current.name == *asset) {
            missing.push("assets", format!("missing {asset} for target {target}"));
        }
    }
    if !missing.errors.is_empty() {
        return Err(missing);
    }
    Ok(())
}

fn publish_draft(
    resolved: &ResolvedConfig,
    forge: &dyn Forge,
    tag: &str,
    expected: &[String],
) -> Result<Vec<PublishedAsset>, DiagnosticReport> {
    let scratch = ScratchDir::new()?;
    let mut assets = download_and_hash(forge, tag, expected, &scratch.0)?;
    let checksums = write_checksums(&assets, &scratch.0)?;
    forge.upload(tag, &[checksums.as_path()])?;
    let refreshed = refresh_draft(forge, tag)?;
    let (artifacts, checksum_url) = release_artifacts(resolved, &refreshed, expected, &assets)?;
    let notes = render_release_notes(resolved, tag, &refreshed.url, &artifacts, &checksum_url)?;
    let notes_path = scratch.0.join("release-notes.md");
    fs::write(&notes_path, notes)
        .map_err(|error| DiagnosticReport::one("release.template", error.to_string()))?;
    forge.update_notes(tag, &notes_path)?;
    forge.undraft(tag)?;
    assets.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(assets)
}

fn download_and_hash(
    forge: &dyn Forge,
    tag: &str,
    expected: &[String],
    scratch: &Path,
) -> Result<Vec<PublishedAsset>, DiagnosticReport> {
    let patterns: Vec<_> = expected.iter().map(String::as_str).collect();
    forge.download(tag, &patterns, scratch)?;
    let mut assets = Vec::new();
    for name in expected {
        let path = scratch.join(name);
        if !path.is_file() {
            return Err(DiagnosticReport::one(
                "assets",
                format!("download did not produce {name}"),
            ));
        }
        let sha256 = sha256_hex(&path)?;
        assets.push(PublishedAsset {
            name: name.clone(),
            sha256,
        });
    }
    Ok(assets)
}

fn write_checksums(assets: &[PublishedAsset], scratch: &Path) -> Result<PathBuf, DiagnosticReport> {
    let mut sums: Vec<_> = assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    sums.sort_by(|left, right| left.0.cmp(&right.0));
    let checksums = scratch.join("SHA256SUMS");
    fs::write(&checksums, sha256sums(&sums))
        .map_err(|error| DiagnosticReport::one("SHA256SUMS", error.to_string()))?;
    Ok(checksums)
}

fn refresh_draft(forge: &dyn Forge, tag: &str) -> Result<ReleaseView, DiagnosticReport> {
    let refreshed = forge
        .view_release(tag)?
        .ok_or_else(|| DiagnosticReport::one("release", format!("release {tag} disappeared")))?;
    if !refreshed.is_draft {
        return Err(DiagnosticReport::one(
            "release",
            "release became published before its description could be updated",
        ));
    }
    Ok(refreshed)
}

fn release_artifacts(
    resolved: &ResolvedConfig,
    refreshed: &ReleaseView,
    expected: &[String],
    assets: &[PublishedAsset],
) -> Result<(Vec<ReleaseArtifact>, String), DiagnosticReport> {
    debug_assert_eq!(resolved.targets.len(), expected.len());
    debug_assert_eq!(expected.len(), assets.len());
    if refreshed.url.is_empty() {
        return Err(DiagnosticReport::one(
            "release",
            "GitHub did not return the release URL",
        ));
    }
    let checksum_asset = refreshed
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .filter(|asset| !asset.url.is_empty())
        .ok_or_else(|| {
            DiagnosticReport::one("assets", "GitHub did not return the SHA256SUMS URL")
        })?;
    let mut artifacts = Vec::new();
    for ((target, expected_name), published) in resolved.targets.iter().zip(expected).zip(assets) {
        let github_asset = refreshed
            .assets
            .iter()
            .find(|asset| asset.name == *expected_name)
            .filter(|asset| !asset.url.is_empty())
            .ok_or_else(|| {
                DiagnosticReport::one(
                    "assets",
                    format!("GitHub did not return the URL for {expected_name}"),
                )
            })?;
        artifacts.push(ReleaseArtifact {
            target: target.clone(),
            name: expected_name.clone(),
            url: github_asset.url.clone(),
            sha256: published.sha256.clone(),
        });
    }
    Ok((artifacts, checksum_asset.url.clone()))
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> Result<Self, DiagnosticReport> {
        let nonce = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("livreur-publish-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).map_err(|error| {
            DiagnosticReport::one(path.display().to_string(), error.to_string())
        })?;
        Ok(Self(path))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
