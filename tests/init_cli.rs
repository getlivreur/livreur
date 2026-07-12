use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use toml::Value;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("livreur-init-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).expect("create fixture");
        Self(path)
    }

    fn config(&self) -> PathBuf {
        self.0.join("nested/livreur.toml")
    }

    fn workflow(&self) -> PathBuf {
        self.0.join("nested/.github/workflows/release.yml")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn init(config: &Path, workflow: &Path, no_workflow: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_livreur"));
    command
        .arg("init")
        .arg("--config")
        .arg(config)
        .arg("--workflow")
        .arg(workflow);
    if no_workflow {
        command.arg("--no-workflow");
    }
    command.output().expect("run livreur")
}

#[test]
fn init_writes_the_default_config() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let workflow = fixture.workflow();
    let output = init(&config, &workflow, false);

    assert!(output.status.success());
    let contents = fs::read_to_string(&config).expect("config written");
    let parsed: Value = toml::from_str(&contents).expect("valid TOML");

    assert_eq!(parsed["schema"].as_integer(), Some(1));
    assert_eq!(parsed["release"]["tag"].as_str(), Some("v{version}"));
    assert_eq!(
        parsed["release"]["targets"]
            .as_array()
            .expect("targets array")
            .len(),
        5
    );
    assert_eq!(parsed["build"]["locked"].as_bool(), Some(true));
    assert_eq!(parsed["installers"]["unix"].as_bool(), Some(true));
    assert_eq!(parsed["installers"]["powershell"].as_bool(), Some(true));
    assert_eq!(parsed["npm"]["enabled"].as_bool(), Some(false));
    assert_eq!(parsed["homebrew"]["enabled"].as_bool(), Some(false));
    let workflow = fs::read_to_string(workflow).expect("workflow written");
    assert!(workflow.contains("permissions:\n  contents: write"));
    assert!(workflow.contains("livreur publish"));
}

#[test]
fn init_refuses_to_overwrite_an_existing_file() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let workflow = fixture.workflow();
    fs::create_dir_all(config.parent().expect("config parent")).expect("create config parent");
    fs::write(&config, "custom = true").expect("write existing file");

    let output = init(&config, &workflow, false);

    assert_eq!(output.status.code(), Some(2));
    let contents = fs::read_to_string(&config).expect("config still present");
    assert_eq!(contents, "custom = true");
    assert!(
        workflow.is_file(),
        "missing workflow should still be created"
    );
}

#[test]
fn init_can_skip_the_workflow() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let workflow = fixture.workflow();

    let output = init(&config, &workflow, true);

    assert!(output.status.success());
    assert!(config.is_file());
    assert!(!workflow.exists());
}

#[test]
fn init_preserves_an_existing_workflow_but_creates_the_config() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let workflow = fixture.workflow();
    fs::create_dir_all(workflow.parent().expect("workflow parent"))
        .expect("create workflow parent");
    fs::write(&workflow, "custom workflow").expect("write workflow");

    let output = init(&config, &workflow, false);

    assert_eq!(output.status.code(), Some(2));
    assert!(config.is_file(), "missing config should still be created");
    assert_eq!(fs::read_to_string(workflow).unwrap(), "custom workflow");
}
