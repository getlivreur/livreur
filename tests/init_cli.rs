use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use toml::Value;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct TempConfig(PathBuf);

impl TempConfig {
    fn new() -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("livreur-init-{}-{nonce}.toml", std::process::id())))
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
    }
}

fn init(config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_livreur"))
        .arg("init")
        .arg("--config")
        .arg(config)
        .output()
        .expect("run livreur")
}

#[test]
fn init_writes_the_default_config() {
    let config = TempConfig::new();
    let output = init(&config.0);

    assert!(output.status.success());
    let contents = fs::read_to_string(&config.0).expect("config written");
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
}

#[test]
fn init_refuses_to_overwrite_an_existing_file() {
    let config = TempConfig::new();
    fs::write(&config.0, "custom = true").expect("write existing file");

    let output = init(&config.0);

    assert_eq!(output.status.code(), Some(2));
    let contents = fs::read_to_string(&config.0).expect("config still present");
    assert_eq!(contents, "custom = true");
}
