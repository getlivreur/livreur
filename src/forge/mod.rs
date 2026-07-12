mod github;

use crate::DiagnosticReport;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseView {
    pub is_draft: bool,
    pub assets: Vec<String>,
}

pub trait Forge {
    /// Looks up a release by tag.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the forge command cannot run or returns invalid data.
    fn view_release(&self, tag: &str) -> Result<Option<ReleaseView>, DiagnosticReport>;

    /// Creates a draft release for an existing tag.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the forge rejects or cannot create the release.
    fn create_draft(&self, tag: &str) -> Result<(), DiagnosticReport>;

    /// Uploads release assets, replacing assets with matching names.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the forge cannot upload the files.
    fn upload(&self, tag: &str, files: &[&Path]) -> Result<(), DiagnosticReport>;

    /// Downloads assets matching `patterns` into `dir`.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the forge cannot download the requested assets.
    fn download(&self, tag: &str, patterns: &[&str], dir: &Path) -> Result<(), DiagnosticReport>;

    /// Publishes a draft release.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the forge cannot update the release.
    fn undraft(&self, tag: &str) -> Result<(), DiagnosticReport>;
}

#[must_use]
pub fn default_forge() -> Box<dyn Forge> {
    Box::new(github::GitHub::default())
}
