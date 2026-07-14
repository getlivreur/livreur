use livreur::DEFAULT_RELEASE_TEMPLATE;
use std::fs;
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
    fs::write(&output_path, DEFAULT_RELEASE_TEMPLATE)
        .map_err(|error| format!("cannot write {}: {error}", output_path.display()))?;
    fs::write(config, document.to_string())
        .map_err(|error| format!("cannot update {}: {error}", config.display()))?;
    Ok(output_path)
}
