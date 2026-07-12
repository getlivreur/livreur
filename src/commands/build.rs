use super::{OutputFormat, eprint_report, print_json, resolve_tag};
use cargo_metadata::Message;
use livreur::archive::{ArchiveEntry, ArchiveKind, create_archive};
use livreur::{
    Config, Diagnostic, DiagnosticReport, asset_name, default_forge, is_windows, sha256_hex,
};
use serde::Serialize;
use std::fs;
use std::io::{BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Serialize)]
struct BuildJsonOutput {
    ok: bool,
    tag: Option<String>,
    target: String,
    asset: Option<String>,
    sha256: Option<String>,
    warnings: Vec<String>,
    errors: Vec<Diagnostic>,
}

pub fn build(
    config: &Path,
    manifest: &Path,
    target: &str,
    tag_flag: Option<&str>,
    format: OutputFormat,
) -> i32 {
    match try_build(config, manifest, target, tag_flag) {
        Ok(success) => {
            match format {
                OutputFormat::Human => {
                    for warning in &success.warnings {
                        eprintln!("warning: {warning}");
                    }
                    println!(
                        "built {} ({}) and uploaded to release {}",
                        success.asset, success.sha256, success.tag
                    );
                }
                OutputFormat::Json => print_json(&BuildJsonOutput {
                    ok: true,
                    tag: Some(success.tag),
                    target: target.to_owned(),
                    asset: Some(success.asset),
                    sha256: Some(success.sha256),
                    warnings: success.warnings,
                    errors: vec![],
                }),
            }
            0
        }
        Err((tag, report)) => {
            match format {
                OutputFormat::Human => eprint_report(&report),
                OutputFormat::Json => print_json(&BuildJsonOutput {
                    ok: false,
                    tag,
                    target: target.to_owned(),
                    asset: None,
                    sha256: None,
                    warnings: vec![],
                    errors: report.errors,
                }),
            }
            2
        }
    }
}

struct BuildSuccess {
    tag: String,
    asset: String,
    sha256: String,
    warnings: Vec<String>,
}

fn try_build(
    config: &Path,
    manifest: &Path,
    target: &str,
    tag_flag: Option<&str>,
) -> Result<BuildSuccess, (Option<String>, DiagnosticReport)> {
    let resolved = Config::load(config, manifest).map_err(|report| (None, report))?;
    let tag = resolve_tag(tag_flag).map_err(|report| (None, report))?;
    resolved
        .for_tag(&tag)
        .map_err(|report| (Some(tag.clone()), report))?;
    if !resolved
        .targets
        .iter()
        .any(|configured| configured == target)
    {
        return Err((
            Some(tag),
            DiagnosticReport::one("--target", format!("target `{target}` is not configured")),
        ));
    }

    let binary =
        cargo_build(&resolved, manifest, target).map_err(|report| (Some(tag.clone()), report))?;
    let manifest_dir = manifest
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (mut entries, warnings) =
        archive_entries(manifest_dir, &binary, &resolved.package.binary, target)
            .map_err(|report| (Some(tag.clone()), report))?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let asset = asset_name(&resolved.package.name, &tag, target);
    let output_dir = manifest_dir.join("target/livreur");
    fs::create_dir_all(&output_dir).map_err(|error| {
        (
            Some(tag.clone()),
            DiagnosticReport::one(output_dir.display().to_string(), error.to_string()),
        )
    })?;
    let archive = output_dir.join(&asset);
    let kind = if is_windows(target) {
        ArchiveKind::Zip
    } else {
        ArchiveKind::TarGz
    };
    create_archive(kind, &entries, &archive).map_err(|report| (Some(tag.clone()), report))?;
    let sha256 = sha256_hex(&archive).map_err(|report| (Some(tag.clone()), report))?;

    let forge = default_forge();
    match forge
        .view_release(&tag)
        .map_err(|report| (Some(tag.clone()), report))?
    {
        None => {
            if let Err(create_error) = forge.create_draft(&tag) {
                match forge.view_release(&tag) {
                    Ok(Some(release)) if release.is_draft => {}
                    Ok(Some(_)) => {
                        return Err((Some(tag.clone()), published_release_error(&tag)));
                    }
                    Ok(None) | Err(_) => return Err((Some(tag), create_error)),
                }
            }
        }
        Some(release) if !release.is_draft => {
            return Err((Some(tag.clone()), published_release_error(&tag)));
        }
        Some(_) => {}
    }
    forge
        .upload(&tag, &[archive.as_path()])
        .map_err(|report| (Some(tag.clone()), report))?;
    Ok(BuildSuccess {
        tag,
        asset,
        sha256,
        warnings,
    })
}

fn published_release_error(tag: &str) -> DiagnosticReport {
    DiagnosticReport::one(
        "release",
        format!(
            "release {tag} is already published; refusing to replace its asset because SHA256SUMS would become stale"
        ),
    )
}

fn cargo_build(
    resolved: &livreur::ResolvedConfig,
    manifest: &Path,
    target: &str,
) -> Result<PathBuf, DiagnosticReport> {
    let program = std::env::var_os("LIVREUR_CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(program);
    command
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg(target)
        .arg("--message-format=json-render-diagnostics")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--bin")
        .arg(&resolved.package.binary);
    if resolved.build.locked {
        command.arg("--locked");
    }
    if resolved.build.no_default_features {
        command.arg("--no-default-features");
    }
    if !resolved.build.features.is_empty() {
        command
            .arg("--features")
            .arg(resolved.build.features.join(","));
    }
    command
        .args(&resolved.build.cargo_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        let message = if error.kind() == ErrorKind::NotFound {
            "cargo executable not found".to_owned()
        } else {
            error.to_string()
        };
        DiagnosticReport::one("cargo", message)
    })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut executable = None;
    for message in Message::parse_stream(BufReader::new(stdout)) {
        match message {
            Ok(Message::CompilerArtifact(artifact))
                if artifact.target.name == resolved.package.binary
                    && artifact.executable.is_some() =>
            {
                executable = artifact
                    .executable
                    .map(cargo_metadata::camino::Utf8PathBuf::into_std_path_buf);
            }
            Ok(_) => {}
            Err(error) => {
                return Err(DiagnosticReport::one(
                    "cargo",
                    format!("invalid Cargo JSON: {error}"),
                ));
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| DiagnosticReport::one("cargo", error.to_string()))?;
    if !status.success() {
        return Err(DiagnosticReport::one(
            "cargo",
            format!("build failed with {status}"),
        ));
    }
    executable.ok_or_else(|| {
        DiagnosticReport::one(
            "cargo",
            format!(
                "build produced no executable for `{}`",
                resolved.package.binary
            ),
        )
    })
}

fn archive_entries(
    manifest_dir: &Path,
    binary: &Path,
    binary_name: &str,
    target: &str,
) -> Result<(Vec<ArchiveEntry>, Vec<String>), DiagnosticReport> {
    let mut entries = vec![ArchiveEntry {
        src: binary.to_owned(),
        name: livreur::bin_file_name(binary_name, target),
        executable: true,
    }];
    let files = fs::read_dir(manifest_dir)
        .map_err(|error| {
            DiagnosticReport::one(manifest_dir.display().to_string(), error.to_string())
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            DiagnosticReport::one(manifest_dir.display().to_string(), error.to_string())
        })?;
    let mut warnings = Vec::new();
    for prefix in ["README", "LICENSE"] {
        let mut found = false;
        for file in &files {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.to_ascii_uppercase().starts_with(prefix) && file.path().is_file() {
                found = true;
                entries.push(ArchiveEntry {
                    src: file.path(),
                    name,
                    executable: false,
                });
            }
        }
        if !found {
            warnings.push(format!(
                "no {prefix}* file found beside the package manifest"
            ));
        }
    }
    Ok((entries, warnings))
}
