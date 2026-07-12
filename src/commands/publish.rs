use super::{OutputFormat, eprint_report, print_json, resolve_tag};
use livreur::checksum::sha256sums;
use livreur::{Config, Diagnostic, DiagnosticReport, default_forge, sha256_hex};
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
    let mut missing = DiagnosticReport { errors: vec![] };
    for (target, asset) in resolved.targets.iter().zip(&expected) {
        if !release.assets.contains(asset) {
            missing.push("assets", format!("missing {asset} for target {target}"));
        }
    }
    if !missing.errors.is_empty() {
        return Err((Some(tag), missing));
    }
    if !release.is_draft {
        return Ok(PublishSuccess {
            tag,
            published: false,
            assets: vec![],
        });
    }

    let scratch = ScratchDir::new().map_err(|report| (Some(tag.clone()), report))?;
    let patterns: Vec<_> = expected.iter().map(String::as_str).collect();
    forge
        .download(&tag, &patterns, &scratch.0)
        .map_err(|report| (Some(tag.clone()), report))?;
    let mut assets = Vec::new();
    for name in &expected {
        let path = scratch.0.join(name);
        if !path.is_file() {
            return Err((
                Some(tag),
                DiagnosticReport::one("assets", format!("download did not produce {name}")),
            ));
        }
        let sha256 = sha256_hex(&path).map_err(|report| (Some(tag.clone()), report))?;
        assets.push(PublishedAsset {
            name: name.clone(),
            sha256,
        });
    }
    assets.sort_by(|left, right| left.name.cmp(&right.name));
    let sums: Vec<_> = assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    let checksums = scratch.0.join("SHA256SUMS");
    fs::write(&checksums, sha256sums(&sums)).map_err(|error| {
        (
            Some(tag.clone()),
            DiagnosticReport::one("SHA256SUMS", error.to_string()),
        )
    })?;
    forge
        .upload(&tag, &[checksums.as_path()])
        .map_err(|report| (Some(tag.clone()), report))?;
    forge
        .undraft(&tag)
        .map_err(|report| (Some(tag.clone()), report))?;
    Ok(PublishSuccess {
        tag,
        published: true,
        assets,
    })
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
