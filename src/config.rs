use cargo_metadata::{MetadataCommand, Package, Target, TargetKind};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

const DEFAULT_TARGETS: [&str; 5] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"schema = 1

# Optional overrides; resolved from Cargo.toml when omitted:
# name, description, license, repository, authors, binary
[package]

[release]
tag = "v{version}"
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
]

[build]
features = []
no_default_features = false
locked = true
cargo_args = []

[installers]
unix = true
powershell = true

[npm]
enabled = false

[homebrew]
enabled = false
"#;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub errors: Vec<Diagnostic>,
}

impl DiagnosticReport {
    fn one(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            errors: vec![Diagnostic {
                path: path.into(),
                message: message.into(),
            }],
        }
    }

    fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            path: path.into(),
            message: message.into(),
        });
    }

    fn finish<T>(self, value: T) -> Result<T, Self> {
        if self.errors.is_empty() {
            Ok(value)
        } else {
            Err(self)
        }
    }
}

impl fmt::Display for DiagnosticReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            writeln!(f, "{}: {}", error.path, error.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for DiagnosticReport {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema: u32,
    #[serde(default)]
    package: RawPackage,
    #[serde(default)]
    release: RawRelease,
    #[serde(default)]
    build: RawBuild,
    #[serde(default)]
    installers: RawInstallers,
    #[serde(default)]
    npm: RawNpm,
    #[serde(default)]
    homebrew: RawHomebrew,
    tool: Option<RawTool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawPackage {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    authors: Option<Vec<String>>,
    binary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRelease {
    tag: String,
    targets: Vec<String>,
}
impl Default for RawRelease {
    fn default() -> Self {
        Self {
            tag: "v{version}".into(),
            targets: DEFAULT_TARGETS.iter().map(|s| (*s).into()).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawBuild {
    features: Vec<String>,
    no_default_features: bool,
    locked: bool,
    cargo_args: Vec<String>,
}
impl Default for RawBuild {
    fn default() -> Self {
        Self {
            features: vec![],
            no_default_features: false,
            locked: true,
            cargo_args: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawInstallers {
    unix: bool,
    powershell: bool,
}
impl Default for RawInstallers {
    fn default() -> Self {
        Self {
            unix: true,
            powershell: true,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawNpm {
    enabled: bool,
    package: Option<String>,
    platform_scope: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawHomebrew {
    enabled: bool,
    tap: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTool {
    version: Version,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    pub description: String,
    pub license: String,
    pub repository: String,
    pub authors: Vec<String>,
    pub binary: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedBuild {
    pub features: Vec<String>,
    pub no_default_features: bool,
    pub locked: bool,
    pub cargo_args: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedInstallers {
    pub unix: bool,
    pub powershell: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedNpm {
    pub enabled: bool,
    pub package: Option<String>,
    pub platform_scope: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedHomebrew {
    pub enabled: bool,
    pub tap: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedTool {
    pub version: Version,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedConfig {
    pub schema: u32,
    pub package: ResolvedPackage,
    pub tag_template: String,
    pub targets: Vec<String>,
    pub build: ResolvedBuild,
    pub installers: ResolvedInstallers,
    pub npm: ResolvedNpm,
    pub homebrew: ResolvedHomebrew,
    pub tool: Option<ResolvedTool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseConfig {
    pub version: Version,
}

pub struct Config;

impl Config {
    /// Loads a Livreur configuration and resolves it against Cargo package metadata.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when either file cannot be read, TOML or Cargo metadata
    /// cannot be parsed, a package cannot be selected, or configuration validation
    /// fails.
    pub fn load(
        config_path: impl AsRef<Path>,
        manifest_path: impl AsRef<Path>,
    ) -> Result<ResolvedConfig, DiagnosticReport> {
        let config_path = config_path.as_ref();
        let text = fs::read_to_string(config_path).map_err(|e| {
            DiagnosticReport::one(
                "config",
                format!("cannot read {}: {e}", config_path.display()),
            )
        })?;
        let raw: RawConfig =
            toml::from_str(&text).map_err(|e| DiagnosticReport::one("config", e.to_string()))?;
        let manifest_path = manifest_path.as_ref();
        let metadata = MetadataCommand::new()
            .manifest_path(manifest_path)
            .exec()
            .map_err(|e| DiagnosticReport::one("Cargo.toml", e.to_string()))?;
        let package = metadata.root_package().or_else(|| if metadata.workspace_members.len() == 1 { metadata.packages.iter().find(|p| p.id == metadata.workspace_members[0]) } else { None })
            .ok_or_else(|| DiagnosticReport::one("package", "could not select one Cargo package; point --manifest-path at the package manifest"))?;
        resolve(raw, package)
    }
}

impl ResolvedConfig {
    /// Resolves the stable release version carried by `tag`.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the tag does not match the configured template,
    /// is not a stable `SemVer` version, or differs from the Cargo package version.
    pub fn for_tag(&self, tag: &str) -> Result<ReleaseConfig, DiagnosticReport> {
        let (prefix, suffix) = split_tag_template(&self.tag_template)?;
        let captured = tag
            .strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(suffix))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                DiagnosticReport::one(
                    "--tag",
                    format!("tag `{tag}` does not match `{}`", self.tag_template),
                )
            })?;
        let version = Version::parse(captured).map_err(|e| {
            DiagnosticReport::one("--tag", format!("captured version is not SemVer: {e}"))
        })?;
        if !version.pre.is_empty() || !version.build.is_empty() {
            return Err(DiagnosticReport::one(
                "--tag",
                "prerelease and build metadata versions are not supported",
            ));
        }
        if version != self.package.version {
            return Err(DiagnosticReport::one(
                "--tag",
                format!(
                    "version {version} does not match Cargo version {}",
                    self.package.version
                ),
            ));
        }
        Ok(ReleaseConfig { version })
    }
}

fn resolve(raw: RawConfig, package: &Package) -> Result<ResolvedConfig, DiagnosticReport> {
    let mut errors = DiagnosticReport { errors: vec![] };
    if raw.schema != 1 {
        errors.push(
            "schema",
            format!("unsupported schema {}; expected 1", raw.schema),
        );
    }
    let (resolved_package, name) = resolve_package(raw.package, package, &mut errors);
    validate_tag_template(&raw.release.tag, &mut errors);
    validate_targets(&raw.release.targets, &mut errors);
    let npm_package = raw
        .npm
        .package
        .or_else(|| Some(name.clone()).filter(|_| raw.npm.enabled));
    if raw.npm.enabled {
        if npm_package.as_deref().is_none_or(str::is_empty) {
            errors.push("npm.package", "is required when npm is enabled");
        }
        match raw.npm.platform_scope.as_deref() {
            Some(s) if s.starts_with('@') && s.len() > 1 => {}
            _ => errors.push(
                "npm.platform_scope",
                "must be an npm scope beginning with `@` when npm is enabled",
            ),
        }
    }
    if raw.homebrew.enabled {
        if let Some(tap) = raw.homebrew.tap.as_deref() {
            validate_slug("homebrew.tap", tap, &mut errors);
        } else {
            errors.push("homebrew.tap", "is required when Homebrew is enabled");
        }
    }
    let tool = raw.tool.map(|tool| ResolvedTool {
        version: tool.version,
    });
    let resolved = ResolvedConfig {
        schema: raw.schema,
        package: resolved_package,
        tag_template: raw.release.tag,
        targets: raw.release.targets,
        build: ResolvedBuild {
            features: raw.build.features,
            no_default_features: raw.build.no_default_features,
            locked: raw.build.locked,
            cargo_args: raw.build.cargo_args,
        },
        installers: ResolvedInstallers {
            unix: raw.installers.unix,
            powershell: raw.installers.powershell,
        },
        npm: ResolvedNpm {
            enabled: raw.npm.enabled,
            package: npm_package,
            platform_scope: raw.npm.platform_scope,
        },
        homebrew: ResolvedHomebrew {
            enabled: raw.homebrew.enabled,
            tap: raw.homebrew.tap,
        },
        tool,
    };
    errors.finish(resolved)
}

fn resolve_package(
    raw: RawPackage,
    package: &Package,
    errors: &mut DiagnosticReport,
) -> (ResolvedPackage, String) {
    let binaries: Vec<&Target> = package
        .targets
        .iter()
        .filter(|t| t.kind.contains(&TargetKind::Bin))
        .collect();
    let binary = match raw.binary.as_deref() {
        Some(name) if binaries.iter().any(|t| t.name == name) => name.to_owned(),
        Some(name) => {
            errors.push(
                "package.binary",
                format!("Cargo package has no binary named `{name}`"),
            );
            name.to_owned()
        }
        None if binaries.len() == 1 => binaries[0].name.clone(),
        None => {
            errors.push(
                "package.binary",
                "must select a binary when Cargo exposes zero or multiple binaries",
            );
            package.name.clone()
        }
    };
    let name = raw.name.unwrap_or_else(|| package.name.clone());
    nonempty("package.name", &name, errors);
    let description = required(
        "package.description",
        raw.description.or_else(|| package.description.clone()),
        errors,
    );
    let license = required(
        "package.license",
        raw.license.or_else(|| package.license.clone()),
        errors,
    );
    let repository = required(
        "package.repository",
        raw.repository.or_else(|| package.repository.clone()),
        errors,
    );
    let authors = raw.authors.unwrap_or_else(|| package.authors.clone());
    if authors.is_empty() {
        errors.push("package.authors", "must resolve to at least one author");
    }
    (
        ResolvedPackage {
            name: name.clone(),
            version: package.version.clone(),
            description,
            license,
            repository,
            authors,
            binary,
        },
        name,
    )
}

fn required(path: &str, value: Option<String>, errors: &mut DiagnosticReport) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => v,
        _ => {
            errors.push(path, "is required either here or in Cargo.toml");
            String::new()
        }
    }
}
fn nonempty(path: &str, value: &str, errors: &mut DiagnosticReport) {
    if value.trim().is_empty() {
        errors.push(path, "must not be empty");
    }
}
fn validate_slug(path: &str, value: &str, errors: &mut DiagnosticReport) {
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        errors.push(path, "must have the form `owner/repository`");
    }
}
fn split_tag_template(template: &str) -> Result<(&str, &str), DiagnosticReport> {
    if template.matches("{version}").count() != 1 {
        return Err(DiagnosticReport::one(
            "release.tag",
            "must contain `{version}` exactly once",
        ));
    }
    let (prefix, suffix) = template.split_once("{version}").expect("count checked");
    if prefix.contains(['{', '}']) || suffix.contains(['{', '}']) {
        return Err(DiagnosticReport::one(
            "release.tag",
            "must not contain braces outside the `{version}` placeholder",
        ));
    }
    Ok((prefix, suffix))
}
fn validate_tag_template(template: &str, errors: &mut DiagnosticReport) {
    if let Err(e) = split_tag_template(template) {
        errors.errors.extend(e.errors);
    }
}
fn validate_targets(targets: &[String], errors: &mut DiagnosticReport) {
    let supported: BTreeSet<_> = DEFAULT_TARGETS.into_iter().collect();
    let mut seen = BTreeSet::new();
    if targets.is_empty() {
        errors.push("release.targets", "must contain at least one target");
    }
    for target in targets {
        if !supported.contains(target.as_str()) {
            errors.push(
                "release.targets",
                format!("unsupported v1 target `{target}`"),
            );
        }
        if !seen.insert(target) {
            errors.push("release.targets", format!("duplicate target `{target}`"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(extra: &str) -> Self {
            Self::with_sections(extra, "")
        }

        fn with_package_name(name: &str) -> Self {
            Self::with_sections("", &format!("name = {name:?}"))
        }

        fn with_sections(extra: &str, package_extra: &str) -> Self {
            let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "livreur-config-{}-{nonce}.toml",
                std::process::id()
            ));
            let contents = format!(
                r#"
schema = 1
{extra}
[package]
{package_extra}
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

    impl AsRef<Path> for Fixture {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_file(&self.0).ok();
        }
    }

    #[test]
    fn default_template_matches_default_impls() {
        let raw: RawConfig =
            toml::from_str(DEFAULT_CONFIG_TEMPLATE).expect("default template parses");

        assert_eq!(raw.schema, 1);
        assert!(raw.package.name.is_none());
        assert!(raw.package.description.is_none());
        assert!(raw.package.license.is_none());
        assert!(raw.package.repository.is_none());
        assert!(raw.package.authors.is_none());
        assert!(raw.package.binary.is_none());
        let release = RawRelease::default();
        assert_eq!(raw.release.tag, release.tag);
        assert_eq!(raw.release.targets, release.targets);
        let build = RawBuild::default();
        assert_eq!(raw.build.features, build.features);
        assert_eq!(raw.build.no_default_features, build.no_default_features);
        assert_eq!(raw.build.locked, build.locked);
        assert_eq!(raw.build.cargo_args, build.cargo_args);
        let installers = RawInstallers::default();
        assert_eq!(raw.installers.unix, installers.unix);
        assert_eq!(raw.installers.powershell, installers.powershell);
        let npm = RawNpm::default();
        assert_eq!(raw.npm.enabled, npm.enabled);
        assert_eq!(raw.npm.package, npm.package);
        assert_eq!(raw.npm.platform_scope, npm.platform_scope);
        let homebrew = RawHomebrew::default();
        assert_eq!(raw.homebrew.enabled, homebrew.enabled);
        assert_eq!(raw.homebrew.tap, homebrew.tap);
        assert!(raw.tool.is_none());
    }

    #[test]
    fn tag_template_requires_one_placeholder() {
        assert!(split_tag_template("v{version}").is_ok());
        assert!(split_tag_template("release-{version}-stable").is_ok());
        assert!(split_tag_template("v1").is_err());
        assert!(split_tag_template("{version}-{version}").is_err());
        assert!(split_tag_template("v{version_suffix}").is_err());
        assert!(split_tag_template("v{{version}}").is_err());
        assert!(split_tag_template("v{version}{suffix}").is_err());
    }
    #[test]
    fn default_targets_are_unique() {
        let set: BTreeSet<_> = DEFAULT_TARGETS.into_iter().collect();
        assert_eq!(set.len(), DEFAULT_TARGETS.len());
    }

    #[test]
    fn public_load_resolves_cargo_and_tag() {
        let path = Fixture::new("");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let resolved = Config::load(&path, manifest).expect("valid config");

        assert_eq!(resolved.package.name, "livreur");
        assert_eq!(resolved.package.version, Version::new(0, 1, 0));
        assert_eq!(resolved.targets.len(), 5);
        let release = resolved.for_tag("v0.1.0").expect("matching tag");
        assert_eq!(release.version, Version::new(0, 1, 0));
        assert!(resolved.installers.unix);
        assert!(resolved.installers.powershell);
        assert!(!resolved.npm.enabled);
        assert!(!resolved.homebrew.enabled);
        assert!(resolved.tool.is_none());
    }

    #[test]
    fn tool_version_is_an_optional_pin() {
        let path = Fixture::new("[tool]\nversion = \"1.2.3\"");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let resolved = Config::load(&path, manifest).expect("valid config");

        assert_eq!(
            resolved.tool.expect("configured tool").version,
            Version::new(1, 2, 3)
        );
    }

    #[test]
    fn public_load_rejects_unknown_keys() {
        let path = Fixture::new("typo = true");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let error = Config::load(&path, manifest).expect_err("unknown key must fail");

        assert!(error.to_string().contains("unknown field `typo`"));
    }

    #[test]
    fn public_load_rejects_an_empty_package_name() {
        let path = Fixture::with_package_name("");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let error = Config::load(&path, manifest).expect_err("empty name must fail");

        assert!(
            error
                .to_string()
                .contains("package.name: must not be empty")
        );
    }

    #[test]
    fn tag_must_equal_cargo_version() {
        let path = Fixture::new("");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let resolved = Config::load(&path, manifest).expect("valid config");

        let error = resolved.for_tag("v0.2.0").expect_err("mismatch must fail");
        assert!(error.to_string().contains("does not match Cargo version"));
    }

    #[test]
    fn prerelease_tags_are_not_supported() {
        let path = Fixture::new("");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let mut resolved = Config::load(&path, manifest).expect("valid config");
        resolved.package.version = Version::parse("0.1.0-rc.1").expect("test version");

        let error = resolved
            .for_tag("v0.1.0-rc.1")
            .expect_err("prerelease must fail");
        assert!(error.to_string().contains("not supported"));
    }
}
