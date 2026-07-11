use super::Forge;
use handlebars::Handlebars;
use serde::Serialize;

const WORKFLOW_TEMPLATE: &str = include_str!("../../templates/github-release.yml.hbs");

pub struct Github;

pub struct WorkflowInput<'a> {
    pub repository: &'a str,
    pub version: &'a str,
    pub protected_environment: &'a str,
    pub targets: &'a [String],
}

impl Forge for Github {
    type WorkflowInput<'a> = WorkflowInput<'a>;

    fn repository_from_remote(&self, remote: &str) -> Option<String> {
        parse_repository(remote)
    }

    fn workflow(&self, input: &Self::WorkflowInput<'_>) -> String {
        render_workflow(input)
    }
}

#[must_use]
pub fn parse_repository(remote: &str) -> Option<String> {
    let value = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = if let Some(path) = value.strip_prefix("git@github.com:") {
        path
    } else if let Some(path) = value.strip_prefix("ssh://git@github.com/") {
        path
    } else if let Some(path) = value.strip_prefix("https://github.com/") {
        path
    } else if let Some(path) = value.strip_prefix("http://github.com/") {
        path
    } else {
        return None;
    };
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

#[derive(Serialize)]
struct WorkflowContext<'a> {
    self_hosting: bool,
    version: &'a str,
    protected_environment: String,
    jobs: Vec<Job>,
    needs: String,
    github_ref_name: &'static str,
    publish_condition: &'static str,
}

#[derive(Serialize)]
struct Job {
    id: &'static str,
    name: &'static str,
    runner: &'static str,
    target: &'static str,
}

fn render_workflow(input: &WorkflowInput<'_>) -> String {
    let definitions = [
        Job {
            id: "linux-x64",
            name: "Linux x86_64",
            runner: "ubuntu-latest",
            target: "x86_64-unknown-linux-gnu",
        },
        Job {
            id: "linux-arm64",
            name: "Linux arm64",
            runner: "ubuntu-latest",
            target: "aarch64-unknown-linux-gnu",
        },
        Job {
            id: "macos-x64",
            name: "macOS x86_64",
            runner: "macos-13",
            target: "x86_64-apple-darwin",
        },
        Job {
            id: "macos-arm64",
            name: "macOS arm64",
            runner: "macos-14",
            target: "aarch64-apple-darwin",
        },
        Job {
            id: "windows-x64",
            name: "Windows x86_64",
            runner: "windows-latest",
            target: "x86_64-pc-windows-msvc",
        },
    ];
    let jobs: Vec<_> = definitions
        .into_iter()
        .filter(|job| input.targets.iter().any(|target| target == job.target))
        .collect();
    let needs = jobs.iter().map(|job| job.id).collect::<Vec<_>>().join(", ");
    let context = WorkflowContext {
        self_hosting: input.repository.eq_ignore_ascii_case("getlivreur/livreur"),
        version: input.version,
        protected_environment: serde_json::to_string(input.protected_environment)
            .expect("string is serializable"),
        jobs,
        needs,
        github_ref_name: "${{ github.ref_name }}",
        publish_condition: "${{ always() && !cancelled() }}",
    };
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars
        .render_template(WORKFLOW_TEMPLATE, &context)
        .expect("embedded GitHub workflow template must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_github_remotes() {
        for remote in [
            "git@github.com:getlivreur/livreur.git",
            "https://github.com/getlivreur/livreur.git",
            "ssh://git@github.com/getlivreur/livreur",
        ] {
            assert_eq!(
                parse_repository(remote).as_deref(),
                Some("getlivreur/livreur")
            );
        }
        assert!(parse_repository("https://gitlab.com/a/b").is_none());
    }

    #[test]
    fn generated_workflow_scopes_permissions() {
        let workflow = render_workflow(&WorkflowInput {
            repository: "example/tool",
            version: "1.2.3",
            protected_environment: "release",
            targets: &["x86_64-unknown-linux-gnu".into()],
        });
        assert_eq!(workflow.matches("contents: write").count(), 1);
        assert_eq!(workflow.matches("id-token: write").count(), 1);
        assert!(workflow.contains("cargo install livreur --version '=1.2.3'"));
        assert!(workflow.contains("environment: \"release\""));
        assert_eq!(workflow.matches("uses: actions/checkout@v6").count(), 2);
        assert!(!workflow.contains("actions/checkout@v4"));
        assert!(!workflow.contains("windows-x64:"));
        assert!(workflow.contains("${{ github.ref_name }}"));
    }
}
