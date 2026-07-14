use livreur::DEFAULT_RELEASE_TEMPLATE;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "livreur-template-cli-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture");
        fs::write(
            root.join("livreur.toml"),
            "# keep this comment\nschema = 1\n\n[release]\ntag = \"v{version}\"\n",
        )
        .expect("write config");
        Self(root)
    }

    fn run(&self, force: bool) -> Output {
        self.run_with("livreur.toml", ".github/release.md.tera", force)
    }

    fn run_with(&self, config: &str, output: &str, force: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_livreur"));
        command
            .current_dir(&self.0)
            .arg("template")
            .arg("release")
            .arg("--config")
            .arg(config)
            .arg("--output")
            .arg(output);
        if force {
            command.arg("--force");
        }
        command.output().expect("run template release")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn extracts_the_default_and_preserves_config_comments() {
    let fixture = Fixture::new();
    let output = fixture.run(false);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join(".github/release.md.tera")).unwrap(),
        DEFAULT_RELEASE_TEMPLATE
    );
    let config = fs::read_to_string(fixture.0.join("livreur.toml")).unwrap();
    assert!(config.contains("# keep this comment"));
    assert!(config.contains("template = \".github/release.md.tera\""));
}

#[test]
fn refuses_conflicts_unless_forced() {
    let fixture = Fixture::new();
    assert!(fixture.run(false).status.success());
    let template = fixture.0.join(".github/release.md.tera");
    fs::write(&template, "custom").unwrap();

    let refused = fixture.run(false);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&template).unwrap(), "custom");

    let forced = fixture.run(true);
    assert!(forced.status.success());
    assert_eq!(
        fs::read_to_string(&template).unwrap(),
        DEFAULT_RELEASE_TEMPLATE
    );
}

#[test]
fn resolves_an_alternate_output_from_the_config_directory() {
    let fixture = Fixture::new();
    let nested = fixture.0.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::rename(fixture.0.join("livreur.toml"), nested.join("custom.toml")).unwrap();

    let output = fixture.run_with("nested/custom.toml", "templates/notes.md.tera", false);

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(nested.join("templates/notes.md.tera")).unwrap(),
        DEFAULT_RELEASE_TEMPLATE
    );
    let config = fs::read_to_string(nested.join("custom.toml")).unwrap();
    assert!(config.contains("template = \"templates/notes.md.tera\""));
}

#[test]
fn errors_before_writing_when_config_is_missing() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("livreur.toml")).unwrap();

    let output = fixture.run(false);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        !Path::new(&fixture.0)
            .join(".github/release.md.tera")
            .exists()
    );
}
