use crate::DiagnosticReport;
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
pub fn render_workflow(targets: &[String], tag_glob: &str) -> Result<String, DiagnosticReport> {
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
        let rendered = render_workflow(&targets, "v*").expect("render workflow");
        assert!(rendered.contains("permissions:\n  contents: write"));
        assert!(rendered.contains("tags: [\"v*\"]"));
        assert!(rendered.contains("aarch64-unknown-linux-gnu, runner: ubuntu-24.04-arm"));
        assert!(rendered.contains("GH_TOKEN: ${{ github.token }}"));
        assert!(rendered.contains("livreur build --target ${{ matrix.target }}"));
        assert!(rendered.contains("publish:"));
    }

    #[test]
    fn renders_only_selected_targets() {
        let targets = vec![
            "x86_64-unknown-linux-gnu".into(),
            "aarch64-apple-darwin".into(),
        ];
        let rendered = render_workflow(&targets, "release-*").expect("render workflow");
        assert!(rendered.contains("x86_64-unknown-linux-gnu"));
        assert!(rendered.contains("aarch64-apple-darwin"));
        assert!(!rendered.contains("windows-msvc"));
    }
}
