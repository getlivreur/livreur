#![cfg(unix)]

use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const TARGETS: [&str; 5] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

struct Fixture {
    root: PathBuf,
    gh: PathBuf,
    gh_log: PathBuf,
    checksums: PathBuf,
    notes: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "livreur-publish-cli-{}-{nonce}",
            std::process::id()
        ));
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
        fs::write(root.join("livreur.toml"), "schema = 1\n").unwrap();
        let gh = root.join("fake-gh");
        write_script(
            &gh,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_LOG"
case "$1 $2" in
  "release view")
    if [ "$GH_MODE" = "none" ]; then
      printf '%s\n' 'release not found' >&2
      exit 1
    fi
    if [ -e "$CHECKSUM_COPY" ]; then
      printf '%s\n' "$GH_REFRESH_JSON"
    else
      printf '%s\n' "$GH_VIEW_JSON"
    fi
    ;;
  "release download")
    shift 2
    dir=''
    patterns=''
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --dir) dir=$2; shift 2 ;;
        --pattern) patterns="$patterns $2"; shift 2 ;;
        *) shift ;;
      esac
    done
    for pattern in $patterns; do
      if [ "$pattern" != "$GH_SKIP" ]; then
        printf 'downloaded %s\n' "$pattern" > "$dir/$pattern"
      fi
    done
    ;;
  "release upload")
    for arg in "$@"; do last=$arg; done
    if [ "$(basename "$last")" = "SHA256SUMS" ]; then
      cp "$last" "$CHECKSUM_COPY"
    fi
    ;;
  "release edit")
    if [ "$4" = "--notes-file" ]; then
      if [ "$GH_MODE" = "notes-error" ]; then
        printf '%s\n' 'could not update release notes' >&2
        exit 1
      fi
      cp "$5" "$NOTES_COPY"
    fi
    ;;
esac
exit 0
"#,
        );
        Self {
            gh_log: root.join("gh.log"),
            checksums: root.join("captured-SHA256SUMS"),
            notes: root.join("captured-release-notes.md"),
            root,
            gh,
        }
    }

    fn expected(&self) -> Vec<String> {
        TARGETS
            .iter()
            .map(|target| {
                let ext = if target.contains("windows") {
                    "zip"
                } else {
                    "tar.gz"
                };
                format!("fixture-v0.1.0-{target}.{ext}")
            })
            .collect()
    }

    fn view_json(&self, draft: bool, include_sums: bool, omit_last: bool) -> String {
        let mut assets = self.expected();
        if omit_last {
            assets.pop();
        }
        if include_sums {
            assets.push("SHA256SUMS".into());
        }
        let assets = assets
            .into_iter()
            .map(|name| {
                let url =
                    format!("https://github.com/example/fixture/releases/download/v0.1.0/{name}");
                json!({ "name": name, "url": url })
            })
            .collect::<Vec<_>>();
        json!({
            "isDraft": draft,
            "url": "https://github.com/example/fixture/releases/tag/v0.1.0",
            "assets": assets
        })
        .to_string()
    }

    fn run(&self, view_json: &str, mode: &str, skip: Option<&str>) -> Output {
        Command::new(env!("CARGO_BIN_EXE_livreur"))
            .arg("publish")
            .arg("--config")
            .arg(self.root.join("livreur.toml"))
            .arg("--manifest-path")
            .arg(self.root.join("Cargo.toml"))
            .arg("--tag")
            .arg("v0.1.0")
            .arg("--format")
            .arg("json")
            .env("LIVREUR_GH", &self.gh)
            .env("GH_LOG", &self.gh_log)
            .env("GH_MODE", mode)
            .env("GH_VIEW_JSON", view_json)
            .env("GH_REFRESH_JSON", self.view_json(true, true, false))
            .env("GH_SKIP", skip.unwrap_or(""))
            .env("CHECKSUM_COPY", &self.checksums)
            .env("NOTES_COPY", &self.notes)
            .output()
            .expect("run livreur")
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

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

#[test]
fn downloads_all_assets_uploads_checksums_and_publishes() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.view_json(true, false, false), "draft", None);
    let body = json_output(&output);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["published"], true);
    assert_eq!(body["assets"].as_array().unwrap().len(), 5);
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.matches("--pattern").count(), 5);
    assert!(log.contains("release upload v0.1.0 --clobber"));
    assert!(log.contains("release edit v0.1.0 --notes-file"));
    assert!(log.contains("release edit v0.1.0 --draft=false"));
    let lines = log.lines().collect::<Vec<_>>();
    let upload = lines
        .iter()
        .position(|line| line.contains("release upload"))
        .unwrap();
    let notes = lines
        .iter()
        .position(|line| line.contains("--notes-file"))
        .unwrap();
    let publish = lines
        .iter()
        .position(|line| line.contains("--draft=false"))
        .unwrap();
    assert!(upload < notes && notes < publish);
    let sums = fs::read_to_string(&fixture.checksums).expect("captured checksums");
    assert_eq!(sums.lines().count(), 5);
    assert!(
        sums.lines()
            .all(|line| line.len() > 66 && line.as_bytes()[64..66] == *b"  ")
    );
    let names = sums.lines().map(|line| &line[66..]).collect::<Vec<_>>();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    let notes = fs::read_to_string(&fixture.notes).expect("captured release notes");
    assert!(notes.contains("fixture"));
    assert!(notes.contains(TARGETS[0]));
    assert!(notes.contains("releases/download/v0.1.0"));
    assert!(notes.contains("SHA256SUMS"));
}

#[test]
fn refuses_missing_release_assets_before_download() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.view_json(true, false, true), "draft", None);

    assert_eq!(output.status.code(), Some(2));
    let body = json_output(&output);
    assert_eq!(body["errors"][0]["path"], "assets");
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains(TARGETS[4])
    );
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert!(!log.contains("release download"));
}

#[test]
fn reports_a_download_that_does_not_land_an_asset() {
    let fixture = Fixture::new();
    let skipped = fixture.expected()[0].clone();
    let output = fixture.run(
        &fixture.view_json(true, false, false),
        "draft",
        Some(&skipped),
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains(&skipped)
    );
}

#[test]
fn a_notes_update_failure_leaves_the_release_as_a_draft() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.view_json(true, false, false), "notes-error", None);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not update release notes")
    );
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert!(log.contains("--notes-file"));
    assert!(!log.contains("--draft=false"));
}

#[test]
fn already_published_with_checksums_is_a_successful_noop() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.view_json(false, true, false), "published", None);
    let body = json_output(&output);

    assert!(output.status.success());
    assert_eq!(body["published"], false);
    assert!(body["checksums"].is_null());
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn already_published_without_checksums_is_not_mutated() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.view_json(false, false, false), "published", None);
    let body = json_output(&output);

    assert!(output.status.success());
    assert_eq!(body["published"], false);
    assert!(body["checksums"].is_null());
    let log = fs::read_to_string(&fixture.gh_log).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(!log.contains("release download"));
    assert!(!log.contains("release upload"));
    assert!(!log.contains("release edit"));
}

#[test]
fn reports_a_missing_release() {
    let fixture = Fixture::new();
    let output = fixture.run("{}", "none", None);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("run `livreur build` first")
    );
}

#[test]
fn publish_validates_the_release_template_before_contacting_github() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("livreur.toml"),
        "schema = 1\n[release]\ntemplate = \"missing-release-template.tera\"\n",
    )
    .unwrap();

    let output = fixture.run(&fixture.view_json(true, false, false), "draft", None);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json_output(&output)["errors"][0]["path"],
        "release.template"
    );
    assert!(!fixture.gh_log.exists());
}
