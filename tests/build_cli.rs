#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const TARGET: &str = "x86_64-unknown-linux-gnu";

struct Fixture {
    root: PathBuf,
    gh: PathBuf,
    cargo: PathBuf,
    gh_log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("livreur-build-cli-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("src")).expect("create fixture");
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "fixture"
version = "0.1.0"
edition = "2024"
description = "fixture"
license = "MIT"
repository = "https://github.com/example/fixture"
authors = ["Fixture"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join("livreur.toml"),
            format!(
                r#"schema = 1
[release]
targets = ["{TARGET}"]
[build]
locked = false
"#
            ),
        )
        .unwrap();
        fs::write(root.join("README.md"), "read me\n").unwrap();
        fs::write(root.join("LICENSE-MIT"), "license\n").unwrap();

        let cargo = root.join("fake-cargo");
        write_script(
            &cargo,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$CARGO_LOG"
printf 'fake executable\n' > "$FAKE_BINARY"
printf '%s\n' "{\"reason\":\"compiler-artifact\",\"package_id\":\"path+file:///fixture#0.1.0\",\"manifest_path\":\"/fixture/Cargo.toml\",\"target\":{\"kind\":[\"bin\"],\"crate_types\":[\"bin\"],\"name\":\"fixture\",\"src_path\":\"/fixture/src/main.rs\",\"edition\":\"2024\",\"doc\":true,\"doctest\":false,\"test\":true},\"profile\":{\"opt_level\":\"3\",\"debuginfo\":0,\"debug_assertions\":false,\"overflow_checks\":false,\"test\":false},\"features\":[],\"filenames\":[\"$FAKE_BINARY\"],\"executable\":\"$FAKE_BINARY\",\"fresh\":false}"
"#,
        );
        let gh = root.join("fake-gh");
        write_script(
            &gh,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_LOG"
if [ "$1 $2" = "release view" ]; then
  if [ "$GH_MODE" = "error" ]; then
    printf '%s\n' 'authentication failed' >&2
    exit 1
  fi
  if [ "$GH_MODE" = "existing" ] || [ "$GH_MODE" = "published" ]; then
    if [ "$GH_MODE" = "published" ]; then draft=false; else draft=true; fi
    printf '{"isDraft":%s,"assets":[]}\n' "$draft"
    exit 0
  fi
  if [ "$GH_MODE" = "race-published" ] && [ -e "$GH_STATE" ]; then
    printf '%s\n' '{"isDraft":false,"assets":[]}'
    exit 0
  fi
  if [ "$GH_MODE" = "race-published" ]; then
    : > "$GH_STATE"
  fi
  printf '%s\n' 'release not found' >&2
  exit 1
fi
if [ "$1 $2" = "release create" ] && [ "$GH_MODE" = "race-published" ]; then
  printf '%s\n' 'release already exists' >&2
  exit 1
fi
exit 0
"#,
        );
        let gh_log = root.join("gh.log");
        Self {
            root,
            gh,
            cargo,
            gh_log,
        }
    }

    fn run(&self, target: &str, tag: Option<&str>, gh: &Path, mode: &str) -> Output {
        self.run_with_format(target, tag, gh, mode, "json")
    }

    fn run_with_format(
        &self,
        target: &str,
        tag: Option<&str>,
        gh: &Path,
        mode: &str,
        format: &str,
    ) -> Output {
        let binary = self.root.join("fake-built-binary");
        let mut command = Command::new(env!("CARGO_BIN_EXE_livreur"));
        command
            .arg("build")
            .arg("--config")
            .arg(self.root.join("livreur.toml"))
            .arg("--manifest-path")
            .arg(self.root.join("Cargo.toml"))
            .arg("--target")
            .arg(target)
            .arg("--format")
            .arg(format)
            .env("LIVREUR_CARGO", &self.cargo)
            .env("LIVREUR_GH", gh)
            .env("GH_LOG", &self.gh_log)
            .env("GH_MODE", mode)
            .env("GH_STATE", self.root.join("gh-state"))
            .env("CARGO_LOG", self.root.join("cargo.log"))
            .env("FAKE_BINARY", binary)
            .env_remove("GITHUB_REF_NAME");
        if let Some(tag) = tag {
            command.arg("--tag").arg(tag);
        }
        command.output().expect("run livreur")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn write_script(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write script");
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

#[test]
fn builds_archives_and_creates_and_uploads_a_draft() {
    let fixture = Fixture::new();
    let output = fixture.run(TARGET, Some("v0.1.0"), &fixture.gh, "missing");
    let body = json(&output);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["tag"], "v0.1.0");
    assert_eq!(body["target"], TARGET);
    assert_eq!(body["asset"], format!("fixture-v0.1.0-{TARGET}.tar.gz"));
    assert_eq!(body["sha256"].as_str().unwrap().len(), 64);
    let archive = fixture
        .root
        .join("target/livreur")
        .join(body["asset"].as_str().unwrap());
    assert!(archive.is_file());
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    let lines = log.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "release view v0.1.0 --json assets,isDraft");
    assert!(lines[1].starts_with("release create v0.1.0 --draft --verify-tag"));
    assert!(lines[2].contains("release upload v0.1.0 --clobber"));
}

#[test]
fn reuses_an_existing_draft() {
    let fixture = Fixture::new();
    let output = fixture.run(TARGET, Some("v0.1.0"), &fixture.gh, "existing");

    assert!(output.status.success());
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert!(!log.contains("release create"));
    assert!(log.contains("release upload"));
}

#[test]
fn refuses_to_replace_an_asset_on_a_published_release() {
    let fixture = Fixture::new();
    let output = fixture.run_with_format(TARGET, Some("v0.1.0"), &fixture.gh, "published", "human");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("release v0.1.0 is already published"));
    assert!(stderr.contains("SHA256SUMS would become stale"));
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(!log.contains("release upload"));
}

#[test]
fn gh_view_errors_are_not_treated_as_a_missing_release() {
    let fixture = Fixture::new();
    let output = fixture.run(TARGET, Some("v0.1.0"), &fixture.gh, "error");
    let body = json(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(body["errors"][0]["path"], "gh");
    assert_eq!(body["errors"][0]["message"], "authentication failed");
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(!log.contains("release create"));
    assert!(!log.contains("release upload"));
}

#[test]
fn draft_create_race_does_not_upload_to_a_release_that_became_public() {
    let fixture = Fixture::new();
    let output = fixture.run(TARGET, Some("v0.1.0"), &fixture.gh, "race-published");
    let body = json(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(body["errors"][0]["path"], "release");
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("SHA256SUMS would become stale")
    );
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.matches("release view").count(), 2);
    assert!(log.contains("release create"));
    assert!(!log.contains("release upload"));
}

#[test]
fn rejects_an_unconfigured_target_before_invoking_tools() {
    let fixture = Fixture::new();
    let output = fixture.run(
        "aarch64-unknown-linux-gnu",
        Some("v0.1.0"),
        &fixture.gh,
        "missing",
    );

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["errors"][0]["path"], "--target");
    assert!(!fixture.gh_log.exists());
    assert!(!fixture.root.join("cargo.log").exists());
}

#[test]
fn resolves_the_tag_from_github_ref_name() {
    let fixture = Fixture::new();
    let binary = fixture.root.join("fake-built-binary");
    let output = Command::new(env!("CARGO_BIN_EXE_livreur"))
        .arg("build")
        .arg("--config")
        .arg(fixture.root.join("livreur.toml"))
        .arg("--manifest-path")
        .arg(fixture.root.join("Cargo.toml"))
        .arg("--target")
        .arg(TARGET)
        .arg("--format")
        .arg("json")
        .env("GITHUB_REF_NAME", "v0.1.0")
        .env("LIVREUR_CARGO", &fixture.cargo)
        .env("LIVREUR_GH", &fixture.gh)
        .env("GH_LOG", &fixture.gh_log)
        .env("GH_MODE", "existing")
        .env("CARGO_LOG", fixture.root.join("cargo.log"))
        .env("FAKE_BINARY", binary)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(json(&output)["tag"], "v0.1.0");
}

#[test]
fn reports_a_missing_tag_and_a_missing_gh_cli() {
    let fixture = Fixture::new();
    let no_tag = fixture.run(TARGET, None, &fixture.gh, "existing");
    assert_eq!(no_tag.status.code(), Some(2));
    assert_eq!(json(&no_tag)["errors"][0]["path"], "--tag");

    let missing_gh = fixture.root.join("does-not-exist");
    let output = fixture.run(TARGET, Some("v0.1.0"), &missing_gh, "missing");
    assert_eq!(output.status.code(), Some(2));
    let body = json(&output);
    assert_eq!(body["errors"][0]["path"], "gh");
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("gh CLI not found")
    );
}
