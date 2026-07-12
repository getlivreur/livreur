use super::{Forge, ReleaseView};
use crate::DiagnosticReport;
use serde::Deserialize;
use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Output};

/// GitHub release operations implemented through `gh`.
///
/// Draft creation is deliberately view-then-create. Concurrent matrix jobs can
/// still race; callers re-view after a create conflict and publishing refuses a
/// partial asset set.
pub struct GitHub {
    program: OsString,
}

impl Default for GitHub {
    fn default() -> Self {
        Self {
            program: std::env::var_os("LIVREUR_GH").unwrap_or_else(|| "gh".into()),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhRelease {
    is_draft: bool,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
}

impl Forge for GitHub {
    fn view_release(&self, tag: &str) -> Result<Option<ReleaseView>, DiagnosticReport> {
        let output = self.run(["release", "view", tag, "--json", "assets,isDraft"])?;
        if !output.status.success() {
            let message = output_message(&output);
            if output.status.code() == Some(1)
                && message.to_ascii_lowercase().contains("release not found")
            {
                return Ok(None);
            }
            return Err(command_error(message));
        }
        let release: GhRelease = serde_json::from_slice(&output.stdout).map_err(|error| {
            DiagnosticReport::one("gh", format!("invalid release JSON: {error}"))
        })?;
        Ok(Some(ReleaseView {
            is_draft: release.is_draft,
            assets: release.assets.into_iter().map(|asset| asset.name).collect(),
        }))
    }

    fn create_draft(&self, tag: &str) -> Result<(), DiagnosticReport> {
        self.run_success([
            "release",
            "create",
            tag,
            "--draft",
            "--verify-tag",
            "--title",
            tag,
            "--notes",
            "",
        ])
    }

    fn upload(&self, tag: &str, files: &[&Path]) -> Result<(), DiagnosticReport> {
        let mut args = vec![
            OsString::from("release"),
            OsString::from("upload"),
            tag.into(),
            "--clobber".into(),
        ];
        args.extend(files.iter().map(|file| file.as_os_str().to_owned()));
        self.run_success(args)
    }

    fn download(&self, tag: &str, patterns: &[&str], dir: &Path) -> Result<(), DiagnosticReport> {
        let mut args = vec![
            OsString::from("release"),
            OsString::from("download"),
            tag.into(),
            "--dir".into(),
            dir.as_os_str().to_owned(),
        ];
        for pattern in patterns {
            args.push("--pattern".into());
            args.push((*pattern).into());
        }
        self.run_success(args)
    }

    fn undraft(&self, tag: &str) -> Result<(), DiagnosticReport> {
        self.run_success(["release", "edit", tag, "--draft=false"])
    }
}

impl GitHub {
    fn run<I, S>(&self, args: I) -> Result<Output, DiagnosticReport>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new(&self.program).args(args).output().map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                DiagnosticReport::one(
                    "gh",
                    "gh CLI not found on PATH; install from https://cli.github.com (in Actions set GH_TOKEN)",
                )
            } else {
                DiagnosticReport::one("gh", error.to_string())
            }
        })
    }

    fn run_success<I, S>(&self, args: I) -> Result<(), DiagnosticReport>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error(output_message(&output)))
        }
    }
}

fn output_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    }
}

fn command_error(message: String) -> DiagnosticReport {
    DiagnosticReport::one("gh", message)
}
