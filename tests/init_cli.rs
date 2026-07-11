use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self::with_binary(true)
    }

    fn library() -> Self {
        Self::with_binary(false)
    }

    fn with_binary(binary: bool) -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("init-cli-{}-{nonce}", std::process::id()));
        fs::create_dir_all(path.join("src")).expect("create fixture");
        fs::write(
            path.join("Cargo.toml"),
            r#"[package]
name = "fixture-cli"
version = "1.0.0"
edition = "2024"
description = "fixture"
license = "MIT"
repository = "https://github.com/example/fixture-cli"
authors = ["Example"]

[workspace]
"#,
        )
        .expect("write manifest");
        let source = if binary { "main.rs" } else { "lib.rs" };
        fs::write(path.join("src").join(source), "fn fixture() {}\n").expect("write source");
        assert!(
            Command::new("git")
                .arg("init")
                .arg("--quiet")
                .current_dir(&path)
                .status()
                .expect("initialize git fixture")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "git@github.com:getlivreur/livreur.git"
                ])
                .current_dir(&path)
                .status()
                .expect("configure fixture remote")
                .success()
        );
        Self(path)
    }

    fn run(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_livreur"))
            .current_dir(&self.0)
            .arg("init")
            .arg("--yes")
            .args(extra)
            .output()
            .expect("run init")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn init_creates_valid_config_and_managed_workflow() {
    let fixture = Fixture::new();
    let output = fixture.run(&[]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(fixture.0.join("livreur.toml")).expect("generated config");
    let workflow = fs::read_to_string(fixture.0.join(".github/workflows/release.yml"))
        .expect("generated workflow");
    assert!(config.contains("[forge.github]"));
    assert!(config.contains("repository = \"getlivreur/livreur\""));
    assert!(workflow.contains("This file is fully managed"));
    assert!(workflow.contains("livreur publish is not implemented yet"));
}

#[test]
fn noninteractive_init_refuses_existing_files_unless_forced() {
    let fixture = Fixture::new();
    assert!(fixture.run(&[]).status.success());
    fs::write(fixture.0.join("livreur.toml"), "user content\n").expect("replace fixture");

    let refused = fixture.run(&[]);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(fixture.0.join("livreur.toml")).expect("preserved file"),
        "user content\n"
    );

    let forced = fixture.run(&["--force"]);
    assert!(forced.status.success());
    assert!(
        fs::read_to_string(fixture.0.join("livreur.toml"))
            .expect("regenerated file")
            .contains("[forge.github]")
    );
}

#[test]
fn init_rejects_library_only_packages_with_an_actionable_error() {
    let fixture = Fixture::library();

    let output = fixture.run(&[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "Cargo package has no binary targets; Livreur v1 requires at least one binary"
        )
    );
    assert!(!fixture.0.join("livreur.toml").exists());
    assert!(!fixture.0.join(".github/workflows/release.yml").exists());
}
