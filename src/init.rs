use crate::Config;
use crate::forge::Forge;
use crate::forge::github::{Github, WorkflowInput};
use cargo_metadata::{MetadataCommand, Package, TargetKind};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGETS: [&str; 5] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub manifest_path: PathBuf,
    pub config_path: PathBuf,
    pub workflow_path: PathBuf,
    pub yes: bool,
    pub force: bool,
}

#[derive(Debug)]
pub struct InitResult {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct InitError(pub String);

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InitError {}

/// Discovers project metadata and writes the selected initialization outputs.
///
/// # Errors
///
/// Returns an error when metadata or required defaults cannot be discovered,
/// generated content is invalid, an existing file cannot be replaced safely,
/// or an output cannot be written.
#[allow(clippy::too_many_lines)]
pub fn initialize(options: &InitOptions) -> Result<InitResult, InitError> {
    let interactive = io::stdin().is_terminal() && !options.yes;
    let metadata = MetadataCommand::new()
        .manifest_path(&options.manifest_path)
        .exec()
        .map_err(|e| InitError(format!("cannot read Cargo metadata: {e}")))?;
    let package = metadata
        .root_package()
        .or_else(|| {
            (metadata.workspace_members.len() == 1).then(|| {
                metadata
                    .packages
                    .iter()
                    .find(|package| package.id == metadata.workspace_members[0])
            })?
        })
        .ok_or_else(|| {
            InitError("could not select one Cargo package; use --manifest-path".into())
        })?;

    let forge = Github;
    let manifest_directory = options
        .manifest_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let remote = git_origin(manifest_directory);
    let detected_slug = remote
        .as_deref()
        .and_then(|remote| forge.repository_from_remote(remote));
    let repository = required_value(
        "GitHub repository (owner/name)",
        detected_slug.as_deref(),
        interactive,
    )?;
    if crate_slug(&repository).is_none() {
        return Err(InitError(
            "GitHub repository must have the form owner/repository".into(),
        ));
    }
    let tag_template = prompt("Tag template", Some("v{version}"), interactive)?;
    if tag_template.matches("{version}").count() != 1 {
        return Err(InitError(
            "tag template must contain {version} exactly once".into(),
        ));
    }
    let environment = prompt("GitHub protected environment", Some("release"), interactive)?;
    let target_default = TARGETS.join(",");
    let targets_value = prompt(
        "Release targets (comma-separated)",
        Some(&target_default),
        interactive,
    )?;
    let targets: Vec<String> = targets_value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if targets.is_empty()
        || targets
            .iter()
            .any(|target| !TARGETS.contains(&target.as_str()))
    {
        return Err(InitError(format!(
            "release targets must be selected from: {}",
            TARGETS.join(", ")
        )));
    }

    let package_overrides = package_overrides(package, &repository, interactive)?;
    let config = render_config(
        &repository,
        &tag_template,
        &environment,
        &targets,
        &package_overrides,
    );
    toml::from_str::<toml::Value>(&config)
        .map_err(|e| InitError(format!("generated invalid TOML: {e}")))?;
    let workflow = forge.workflow(&WorkflowInput {
        repository: &repository,
        version: env!("CARGO_PKG_VERSION"),
        protected_environment: &environment,
        targets: &targets,
    });

    let mut selected = Vec::new();
    for (path, contents) in [
        (&options.config_path, config.as_str()),
        (&options.workflow_path, workflow.as_str()),
    ] {
        if !path.exists() || options.force {
            selected.push((path.clone(), contents));
        } else if interactive {
            if confirm(&format!("Replace {}?", path.display()))? {
                selected.push((path.clone(), contents));
            }
        } else {
            return Err(InitError(format!(
                "{} already exists; rerun with --force to replace generated files",
                path.display()
            )));
        }
    }

    // Validate against the selected Cargo package before any output is replaced.
    Config::resolve_text(&config, package)
        .map_err(|report| InitError(format!("generated configuration is invalid:\n{report}")))?;

    let mut written = Vec::new();
    for (path, contents) in selected {
        atomic_write(&path, contents)?;
        written.push(path);
    }
    let skipped = [&options.config_path, &options.workflow_path]
        .into_iter()
        .filter(|path| !written.contains(path))
        .cloned()
        .collect();
    Ok(InitResult { written, skipped })
}

#[derive(Default)]
struct PackageOverrides {
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    authors: Option<Vec<String>>,
    binary: Option<String>,
}

fn package_overrides(
    package: &Package,
    github: &str,
    interactive: bool,
) -> Result<PackageOverrides, InitError> {
    let description = if package.description.as_deref().is_none_or(str::is_empty) {
        Some(required_value("Package description", None, interactive)?)
    } else {
        None
    };
    let license = if package.license.as_deref().is_none_or(str::is_empty) {
        Some(required_value("Package license", None, interactive)?)
    } else {
        None
    };
    let repository = if package.repository.as_deref().is_none_or(str::is_empty) {
        Some(prompt(
            "Package repository URL",
            Some(&format!("https://github.com/{github}")),
            interactive,
        )?)
    } else {
        None
    };
    let authors = if package.authors.is_empty() {
        let value = required_value("Package authors (comma-separated)", None, interactive)?;
        Some(
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    } else {
        None
    };
    let binaries: Vec<_> = package
        .targets
        .iter()
        .filter(|target| target.kind.contains(&TargetKind::Bin))
        .collect();
    let binary = match binaries.as_slice() {
        [] => {
            return Err(InitError(
                "Cargo package has no binary targets; Livreur v1 requires at least one binary"
                    .into(),
            ));
        }
        [_] => None,
        _ => {
            let names = binaries
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let value = required_value(&format!("Cargo binary ({names})"), None, interactive)?;
            if !binaries.iter().any(|target| target.name == value) {
                return Err(InitError(format!(
                    "Cargo package has no binary named {value:?}"
                )));
            }
            Some(value)
        }
    };
    Ok(PackageOverrides {
        description,
        license,
        repository,
        authors,
        binary,
    })
}

#[allow(clippy::too_many_lines)]
fn render_config(
    repository: &str,
    tag: &str,
    environment: &str,
    targets: &[String],
    package: &PackageOverrides,
) -> String {
    let mut output = String::from("# Generated by livreur init.\nschema = 1\n\n[package]\n");
    if let Some(value) = &package.description {
        writeln!(output, "description = {}", quoted(value))
            .expect("writing to a string cannot fail");
    }
    if let Some(value) = &package.license {
        writeln!(output, "license = {}", quoted(value)).expect("writing to a string cannot fail");
    }
    if let Some(value) = &package.repository {
        writeln!(output, "repository = {}", quoted(value))
            .expect("writing to a string cannot fail");
    }
    if let Some(values) = &package.authors {
        writeln!(
            output,
            "authors = [{}]",
            values
                .iter()
                .map(|v| quoted(v))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .expect("writing to a string cannot fail");
    }
    if let Some(value) = &package.binary {
        writeln!(output, "binary = {}", quoted(value)).expect("writing to a string cannot fail");
    }
    write!(
        output,
        "\n[release]\ntag = {}\ntargets = [{}]\n",
        quoted(tag),
        targets
            .iter()
            .map(|v| quoted(v))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .expect("writing to a string cannot fail");
    output.push_str(
        "\n[build]\nfeatures = []\nno_default_features = false\nlocked = true\ncargo_args = []\n",
    );
    output.push_str("\n[installers]\nunix = true\npowershell = true\n");
    write!(
        output,
        "\n[forge.github]\nrepository = {}\nprotected_environment = {}\n",
        quoted(repository),
        quoted(environment)
    )
    .expect("writing to a string cannot fail");
    output.push_str("\n[npm]\nenabled = false\n\n[homebrew]\nenabled = false\n");
    write!(
        output,
        "\n[tool]\nversion = {}\n",
        quoted(env!("CARGO_PKG_VERSION"))
    )
    .expect("writing to a string cannot fail");
    output
}

fn quoted(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn prompt(label: &str, default: Option<&str>, interactive: bool) -> Result<String, InitError> {
    if !interactive {
        return default
            .map(str::to_owned)
            .ok_or_else(|| InitError(format!("cannot infer {label}; rerun interactively")));
    }
    print!(
        "{label}{}: ",
        default.map(|v| format!(" [{v}]")).unwrap_or_default()
    );
    io::stdout().flush().map_err(|e| InitError(e.to_string()))?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|e| InitError(e.to_string()))?;
    let value = value.trim();
    if value.is_empty() {
        default
            .map(str::to_owned)
            .ok_or_else(|| InitError(format!("{label} is required")))
    } else {
        Ok(value.to_owned())
    }
}

fn required_value(
    label: &str,
    default: Option<&str>,
    interactive: bool,
) -> Result<String, InitError> {
    prompt(label, default, interactive)
}

fn confirm(label: &str) -> Result<bool, InitError> {
    Ok(matches!(
        prompt(label, Some("N"), true)?
            .to_ascii_lowercase()
            .as_str(),
        "y" | "yes"
    ))
}

fn git_origin(directory: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(directory)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn crate_slug(value: &str) -> Option<(&str, &str)> {
    let (owner, repo) = value.split_once('/')?;
    (!owner.is_empty() && !repo.is_empty() && !repo.contains('/')).then_some((owner, repo))
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), InitError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|e| InitError(format!("cannot create {}: {e}", parent.display())))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| InitError(format!("invalid output path {}", path.display())))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.livreur.tmp"));
    fs::write(&temporary, contents)
        .map_err(|e| InitError(format!("cannot write {}: {e}", temporary.display())))?;
    fs::rename(&temporary, path)
        .map_err(|e| InitError(format!("cannot replace {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_deterministic_and_forge_scoped() {
        let config = render_config(
            "example/tool",
            "v{version}",
            "release",
            &TARGETS.map(str::to_owned),
            &PackageOverrides::default(),
        );
        assert!(config.contains("[forge.github]\nrepository = \"example/tool\""));
        assert!(config.contains("[tool]\nversion = \"0.1.0\""));
        assert!(!config.contains("\n[github]"));
        toml::from_str::<toml::Value>(&config).expect("valid TOML");
    }
}
