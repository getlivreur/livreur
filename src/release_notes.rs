use crate::{DiagnosticReport, ResolvedConfig};
use serde::Serialize;
use std::fs;
use tera::{Context, Tera};

pub const DEFAULT_RELEASE_TEMPLATE: &str = include_str!("../templates/github-release.md.tera");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseArtifact {
    pub target: String,
    pub name: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Serialize)]
struct TemplateContext<'a> {
    package: &'a crate::config::ResolvedPackage,
    release: TemplateRelease<'a>,
    platforms: Vec<TemplatePlatform<'a>>,
    channels: TemplateChannels<'a>,
}

#[derive(Serialize)]
struct TemplateRelease<'a> {
    tag: &'a str,
    version: &'a semver::Version,
    url: &'a str,
    checksums: TemplateAsset<'a>,
}

#[derive(Serialize)]
struct TemplatePlatform<'a> {
    target: &'a str,
    architecture: &'a str,
    os: &'static str,
    asset: TemplateArtifact<'a>,
}

#[derive(Serialize)]
struct TemplateArtifact<'a> {
    name: &'a str,
    url: &'a str,
    sha256: &'a str,
}

#[derive(Serialize)]
struct TemplateAsset<'a> {
    name: &'a str,
    url: &'a str,
}

#[derive(Serialize)]
struct TemplateChannels<'a> {
    installers: &'a crate::config::ResolvedInstallers,
    npm: &'a crate::config::ResolvedNpm,
    homebrew: &'a crate::config::ResolvedHomebrew,
    crates: &'a crate::config::ResolvedCrates,
}

/// Renders the configured GitHub release description.
///
/// # Errors
///
/// Returns a `release.template` diagnostic when a custom template cannot be
/// read or when Tera cannot parse or render the template.
pub fn render_release_notes(
    config: &ResolvedConfig,
    tag: &str,
    release_url: &str,
    artifacts: &[ReleaseArtifact],
    checksums_url: &str,
) -> Result<String, DiagnosticReport> {
    let template = load_template(config)?;
    render(
        config,
        &template,
        tag,
        release_url,
        artifacts,
        checksums_url,
    )
}

/// Checks that the configured release template can render with representative data.
///
/// # Errors
///
/// Returns a `release.template` diagnostic when the template cannot be read,
/// parsed, or rendered with the stable template context.
pub fn validate_release_template(config: &ResolvedConfig) -> Result<(), DiagnosticReport> {
    let tag = config
        .tag_template
        .replace("{version}", &config.package.version.to_string());
    let artifacts = config
        .targets
        .iter()
        .map(|target| ReleaseArtifact {
            target: target.clone(),
            name: crate::asset_name(&config.package.name, &tag, target),
            url: format!("https://example.invalid/download/{target}"),
            sha256: "0".repeat(64),
        })
        .collect::<Vec<_>>();
    let template = load_template(config)?;
    render(
        config,
        &template,
        &tag,
        "https://example.invalid/releases/tag/example",
        &artifacts,
        "https://example.invalid/download/SHA256SUMS",
    )
    .map(|_| ())
}

fn load_template(config: &ResolvedConfig) -> Result<String, DiagnosticReport> {
    match &config.release_template {
        Some(path) => fs::read_to_string(path).map_err(|error| {
            DiagnosticReport::one(
                "release.template",
                format!("cannot read {}: {error}", path.display()),
            )
        }),
        None => Ok(DEFAULT_RELEASE_TEMPLATE.to_owned()),
    }
}

fn render(
    config: &ResolvedConfig,
    template: &str,
    tag: &str,
    release_url: &str,
    artifacts: &[ReleaseArtifact],
    checksums_url: &str,
) -> Result<String, DiagnosticReport> {
    let platforms = artifacts
        .iter()
        .map(|artifact| {
            let (architecture, os) = platform_parts(&artifact.target);
            TemplatePlatform {
                target: &artifact.target,
                architecture,
                os,
                asset: TemplateArtifact {
                    name: &artifact.name,
                    url: &artifact.url,
                    sha256: &artifact.sha256,
                },
            }
        })
        .collect();
    let context = TemplateContext {
        package: &config.package,
        release: TemplateRelease {
            tag,
            version: &config.package.version,
            url: release_url,
            checksums: TemplateAsset {
                name: "SHA256SUMS",
                url: checksums_url,
            },
        },
        platforms,
        channels: TemplateChannels {
            installers: &config.installers,
            npm: &config.npm,
            homebrew: &config.homebrew,
            crates: &config.crates,
        },
    };
    let context = Context::from_serialize(&context)
        .map_err(|error| DiagnosticReport::one("release.template", error.to_string()))?;
    Tera::one_off(template, &context, false)
        .map_err(|error| DiagnosticReport::one("release.template", error.to_string()))
}

fn platform_parts(target: &str) -> (&str, &'static str) {
    let architecture = target.split('-').next().unwrap_or(target);
    let os = if target.contains("windows") {
        "windows"
    } else if target.contains("android") {
        "android"
    } else if target.contains("linux") {
        "linux"
    } else if target.contains("darwin") {
        "macos"
    } else if target.contains("ios") {
        "ios"
    } else if target.contains("freebsd") {
        "freebsd"
    } else if target.contains("netbsd") {
        "netbsd"
    } else if target.contains("openbsd") {
        "openbsd"
    } else if target.contains("wasi") {
        "wasi"
    } else if target.contains("none") {
        "none"
    } else {
        "unknown"
    };
    (architecture, os)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, DEFAULT_CONFIG_TEMPLATE};
    use std::fs;
    use std::path::Path;

    #[test]
    fn derives_useful_parts_for_targets_outside_the_defaults() {
        assert_eq!(
            platform_parts("riscv64gc-unknown-linux-gnu"),
            ("riscv64gc", "linux")
        );
        assert_eq!(
            platform_parts("aarch64-pc-windows-msvc"),
            ("aarch64", "windows")
        );
        assert_eq!(platform_parts("custom-target"), ("custom", "unknown"));
    }

    #[test]
    fn default_template_renders_downloads_and_checksums() {
        let config_path = std::env::temp_dir().join(format!(
            "livreur-release-template-{}.toml",
            std::process::id()
        ));
        let config_contents = DEFAULT_CONFIG_TEMPLATE.replacen(
            "[package]\n",
            "[package]\nauthors = [\"Example\"]\n",
            1,
        );
        fs::write(&config_path, config_contents).expect("write config");
        let mut config = Config::load(
            &config_path,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        )
        .expect("load config");
        fs::remove_file(config_path).ok();
        let artifact = ReleaseArtifact {
            target: "x86_64-unknown-linux-gnu".into(),
            name: "livreur-v0.0.0-x86_64-unknown-linux-gnu.tar.gz".into(),
            url: "https://example.invalid/linux".into(),
            sha256: "abc123".into(),
        };

        let rendered = render_release_notes(
            &config,
            "v0.0.0",
            "https://example.invalid/release",
            &[artifact],
            "https://example.invalid/SHA256SUMS",
        )
        .expect("render notes");

        assert!(rendered.contains(&config.package.description));
        assert!(rendered.contains("https://example.invalid/linux"));
        assert!(rendered.contains("`abc123`"));
        assert!(rendered.contains("https://example.invalid/SHA256SUMS"));

        let custom_path = std::env::temp_dir().join(format!(
            "livreur-custom-release-template-{}.tera",
            std::process::id()
        ));
        fs::write(
            &custom_path,
            r#"{{ package.name }}|{{ release.tag }}|{{ release.version }}|{{ release.url }}|{{ release.checksums.name }}|{% for platform in platforms %}{{ platform.target }}:{{ platform.architecture }}:{{ platform.os }}:{{ platform.asset.name }}:{{ platform.asset.url }}:{{ platform.asset.sha256 }}{% endfor %}|{{ channels.installers.unix }}|{{ channels.npm.enabled }}|{{ channels.homebrew.enabled }}|{{ channels.crates.enabled }}"#,
        )
        .expect("write custom template");
        config.release_template = Some(custom_path.clone());

        let rendered = render_release_notes(
            &config,
            "v0.0.0",
            "https://example.invalid/release",
            &[ReleaseArtifact {
                target: "x86_64-unknown-linux-gnu".into(),
                name: "archive.tar.gz".into(),
                url: "https://example.invalid/archive".into(),
                sha256: "abc123".into(),
            }],
            "https://example.invalid/SHA256SUMS",
        )
        .expect("render custom notes");

        assert!(rendered.contains("livreur|v0.0.0|0.0.0|https://example.invalid/release"));
        assert!(rendered.contains("x86_64-unknown-linux-gnu:x86_64:linux:archive.tar.gz"));
        assert!(rendered.ends_with("|true|false|false|false"));
        fs::remove_file(custom_path).ok();
    }
}
