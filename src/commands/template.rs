use livreur::DEFAULT_RELEASE_TEMPLATE;
use std::fs;
use std::io;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, value};

pub fn release(config: &Path, output: &Path, force: bool) -> i32 {
    match try_release(config, output, force) {
        Ok(output) => {
            println!(
                "created {} and configured {}",
                output.display(),
                config.display()
            );
            0
        }
        Err(message) => {
            eprintln!("cannot extract release template: {message}");
            2
        }
    }
}

fn try_release(config: &Path, output: &Path, force: bool) -> Result<std::path::PathBuf, String> {
    let config_text = fs::read_to_string(config)
        .map_err(|error| format!("cannot read {}: {error}", config.display()))?;
    let mut document = config_text
        .parse::<DocumentMut>()
        .map_err(|error| format!("cannot parse {}: {error}", config.display()))?;
    let config_dir = config
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let output_path = if output.is_absolute() {
        output.to_owned()
    } else {
        config_dir.join(output)
    };
    let configured = document
        .get("release")
        .and_then(Item::as_table_like)
        .and_then(|release| release.get("template"))
        .is_some_and(|item| !item.is_none());

    if !force {
        let mut conflicts = Vec::new();
        if output_path.exists() {
            conflicts.push(format!("{} already exists", output_path.display()));
        }
        if configured {
            conflicts.push("release.template is already configured".to_owned());
        }
        if !conflicts.is_empty() {
            return Err(format!(
                "{}; pass --force to replace it",
                conflicts.join(" and ")
            ));
        }
    }

    let output_value = output
        .to_str()
        .ok_or_else(|| "output path is not valid UTF-8".to_owned())?;
    if !document.as_table().contains_key("release") {
        document["release"] = Item::Table(Table::new());
    }
    let release = document["release"]
        .as_table_like_mut()
        .ok_or_else(|| "release must be a TOML table".to_owned())?;
    release.insert("template", value(output_value));

    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let previous_output = if output_path.exists() {
        Some(fs::read(&output_path).map_err(|error| {
            format!(
                "cannot preserve existing {} before replacement: {error}",
                output_path.display()
            )
        })?)
    } else {
        None
    };
    let config_contents = document.to_string();
    write_template_and_config(&output_path, previous_output.as_deref(), config, || {
        fs::write(config, config_contents)
    })?;
    Ok(output_path)
}

fn write_template_and_config(
    output: &Path,
    previous_output: Option<&[u8]>,
    config: &Path,
    write_config: impl FnOnce() -> io::Result<()>,
) -> Result<(), String> {
    fs::write(output, DEFAULT_RELEASE_TEMPLATE)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    if let Err(error) = write_config() {
        let message = format!("cannot update {}: {error}", config.display());
        return match restore_output(output, previous_output) {
            Ok(()) => Err(message),
            Err(rollback_error) => Err(format!(
                "{message}; additionally could not restore {}: {rollback_error}",
                output.display()
            )),
        };
    }
    Ok(())
}

fn restore_output(output: &Path, previous_output: Option<&[u8]>) -> io::Result<()> {
    match previous_output {
        Some(contents) => fs::write(output, contents),
        None => fs::remove_file(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn config_write_failure_removes_a_new_template() {
        let root = fixture_dir();
        let output = root.join("release.md.tera");
        let config = root.join("livreur.toml");

        let error = write_template_and_config(&output, None, &config, injected_failure)
            .expect_err("config write must fail");

        assert!(error.contains("injected config failure"));
        assert!(!output.exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn config_write_failure_restores_a_replaced_template() {
        let root = fixture_dir();
        let output = root.join("release.md.tera");
        let config = root.join("livreur.toml");
        fs::write(&output, "custom template").expect("write existing template");
        let previous = fs::read(&output).expect("read existing template");

        let error = write_template_and_config(&output, Some(&previous), &config, injected_failure)
            .expect_err("config write must fail");

        assert!(error.contains("injected config failure"));
        assert_eq!(fs::read_to_string(&output).unwrap(), "custom template");
        fs::remove_dir_all(root).ok();
    }

    fn fixture_dir() -> std::path::PathBuf {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "livreur-template-rollback-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture");
        root
    }

    fn injected_failure() -> io::Result<()> {
        Err(io::Error::other("injected config failure"))
    }
}
