use crate::{DiagnosticReport, ToolVersion};
use serde::Serialize;
use tera::{Context, Tera};

const TEMPLATE: &str = include_str!("../templates/release.yml.tera");

#[must_use]
pub fn runner_for(target: &str) -> &'static str {
    match target {
        "aarch64-unknown-linux-gnu" => "ubuntu-24.04-arm",
        "x86_64-apple-darwin" => "macos-13",
        "aarch64-apple-darwin" => "macos-14",
        "x86_64-pc-windows-msvc" => "windows-latest",
        _ => "ubuntu-24.04",
    }
}

#[derive(Serialize)]
struct WorkflowTarget<'a> {
    triple: &'a str,
    runner: &'static str,
}

/// Renders a self-contained GitHub Actions release workflow.
///
/// # Errors
///
/// Returns a workflow diagnostic if the embedded Tera template cannot be
/// rendered with the supplied targets and tag glob.
pub fn render_workflow(
    targets: &[String],
    tag_glob: &str,
    tool_version: Option<&ToolVersion>,
) -> Result<String, DiagnosticReport> {
    let targets: Vec<_> = targets
        .iter()
        .map(|triple| WorkflowTarget {
            triple,
            runner: runner_for(triple),
        })
        .collect();
    let mut context = Context::new();
    context.insert("targets", &targets);
    context.insert("tag_glob", tag_glob);
    context.insert("tool_version", &tool_version.map(ToString::to_string));
    context.insert(
        "tool_source",
        &tool_version.is_some_and(ToolVersion::is_source),
    );
    Tera::one_off(TEMPLATE, &context, false)
        .map_err(|error| DiagnosticReport::one("workflow", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_TARGETS;

    #[test]
    fn renders_release_workflow() {
        let targets = DEFAULT_TARGETS
            .iter()
            .map(|target| (*target).to_owned())
            .collect::<Vec<_>>();
        let rendered = render_workflow(&targets, "v*", None).expect("render workflow");
        assert!(rendered.contains("permissions:\n  contents: write"));
        assert!(rendered.contains("tags: [\"v*\"]"));
        assert!(rendered.contains("aarch64-unknown-linux-gnu, runner: ubuntu-24.04-arm"));
        assert!(rendered.contains("GH_TOKEN: ${{ github.token }}"));
        assert!(rendered.contains("livreur build --target ${{ matrix.target }}"));
        assert!(rendered.contains("uses: getlivreur/setup-livreur@v1"));
        assert!(!rendered.contains("cargo install livreur"));
        assert!(!rendered.contains("with:\n          version:"));
        assert!(rendered.contains("publish:"));
    }

    #[test]
    fn renders_only_selected_targets() {
        let targets = vec![
            "x86_64-unknown-linux-gnu".into(),
            "aarch64-apple-darwin".into(),
        ];
        let rendered = render_workflow(&targets, "release-*", None).expect("render workflow");
        assert!(rendered.contains("x86_64-unknown-linux-gnu"));
        assert!(rendered.contains("aarch64-apple-darwin"));
        assert!(!rendered.contains("windows-msvc"));
    }

    #[test]
    fn renders_a_pinned_tool_version() {
        let targets = vec!["x86_64-unknown-linux-gnu".into()];
        let version = ToolVersion::Version(semver::Version::new(1, 2, 3));

        let rendered = render_workflow(&targets, "v*", Some(&version)).expect("render workflow");

        assert_eq!(rendered.matches("version: \"1.2.3\"").count(), 2);
        assert!(!rendered.contains("dtolnay/rust-toolchain"));
    }

    #[test]
    fn source_tool_version_sets_up_rust_first() {
        let targets = vec!["x86_64-unknown-linux-gnu".into()];

        let rendered =
            render_workflow(&targets, "v*", Some(&ToolVersion::Source)).expect("render workflow");

        assert_eq!(
            rendered
                .matches("uses: dtolnay/rust-toolchain@stable")
                .count(),
            2
        );
        assert_eq!(rendered.matches("version: \"source\"").count(), 2);
        let rust = rendered
            .match_indices("uses: dtolnay/rust-toolchain@stable")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let setup = rendered
            .match_indices("uses: getlivreur/setup-livreur@v1")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let target = rendered.find("run: rustup target add").unwrap();
        assert_eq!(rust.len(), 2);
        assert_eq!(setup.len(), 2);
        assert!(rust.iter().zip(&setup).all(|(rust, setup)| rust < setup));
        assert!(setup[0] < target);
    }
}
