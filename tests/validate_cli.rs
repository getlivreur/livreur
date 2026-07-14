use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(extra: &str) -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "livreur-cli-config-{}-{nonce}.toml",
            std::process::id()
        ));
        let contents = format!(
            r#"
schema = 1
{extra}
[package]
description = "test package"
license = "MIT"
repository = "https://github.com/example/livreur"
authors = ["Example"]
"#
        );
        fs::write(&path, contents).expect("write fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
    }
}

fn validate(config: &Path, tag: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_livreur"));
    command
        .arg("validate")
        .arg("--config")
        .arg(config)
        .arg("--manifest-path")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .arg("--format")
        .arg("json");
    if let Some(tag) = tag {
        command.arg("--tag").arg(tag);
    }
    command.output().expect("run livreur")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

#[test]
fn valid_config_without_tag_has_no_release() {
    let fixture = Fixture::new("");
    let output = validate(&fixture.0, None);
    let body = json(&output);

    assert!(output.status.success());
    assert_eq!(body["valid"], true);
    assert!(body["resolved"].is_object());
    assert!(body["release"].is_null());
}

#[test]
fn valid_tag_includes_tag_derived_release() {
    let fixture = Fixture::new("");
    let version = env!("CARGO_PKG_VERSION");
    let tag = format!("v{version}");
    let output = validate(&fixture.0, Some(&tag));
    let body = json(&output);

    assert!(output.status.success());
    assert_eq!(body["valid"], true);
    assert_eq!(body["release"]["version"], version);
}

#[test]
fn invalid_tag_preserves_resolved_config() {
    let fixture = Fixture::new("");
    let output = validate(&fixture.0, Some("v0.2.0"));
    let body = json(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(body["valid"], false);
    assert_eq!(body["schema"], 1);
    assert!(body["resolved"].is_object());
    assert!(body["release"].is_null());
    assert_eq!(body["errors"][0]["path"], "--tag");
}

#[test]
fn invalid_config_has_no_resolved_config() {
    let fixture = Fixture::new("typo = true");
    let output = validate(&fixture.0, None);
    let body = json(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(body["valid"], false);
    assert!(body["schema"].is_null());
    assert!(body["resolved"].is_null());
    assert!(body["release"].is_null());
}

#[test]
fn validate_reports_a_missing_release_template() {
    let fixture = Fixture::new("[release]\ntemplate = \"missing-livreur-release-template.tera\"");

    let output = validate(&fixture.0, None);
    let body = json(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(body["errors"][0]["path"], "release.template");
    assert!(body["resolved"].is_object());
}
