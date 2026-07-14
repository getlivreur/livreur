pub mod archive;
pub mod assets;
pub mod checksum;
pub mod config;
pub mod forge;
pub mod release_notes;
pub mod workflow;

pub use assets::{archive_ext, asset_name, bin_file_name, is_windows};
pub use checksum::sha256_hex;
pub use config::{
    Config, DEFAULT_CONFIG_TEMPLATE, DEFAULT_TARGETS, Diagnostic, DiagnosticReport, ReleaseConfig,
    ResolvedConfig, ToolVersion, WorkflowConfig,
};
pub use forge::{Forge, ReleaseAsset, ReleaseView, default_forge};
pub use release_notes::{
    DEFAULT_RELEASE_TEMPLATE, ReleaseArtifact, render_release_notes, validate_release_template,
};
pub use workflow::render_workflow;
