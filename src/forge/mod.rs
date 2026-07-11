pub mod github;

/// Forge-specific rendering and repository discovery.
pub trait Forge {
    type WorkflowInput<'a>;

    fn repository_from_remote(&self, remote: &str) -> Option<String>;
    fn workflow(&self, input: &Self::WorkflowInput<'_>) -> String;
}

#[cfg(test)]
mod tests {
    use super::Forge;

    struct OtherForge;

    impl Forge for OtherForge {
        type WorkflowInput<'a> = usize;

        fn repository_from_remote(&self, remote: &str) -> Option<String> {
            Some(remote.to_owned())
        }

        fn workflow(&self, input: &Self::WorkflowInput<'_>) -> String {
            input.to_string()
        }
    }

    #[test]
    fn forge_controls_its_workflow_input_type() {
        assert_eq!(OtherForge.workflow(&42), "42");
    }
}
