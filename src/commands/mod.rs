pub mod build;
pub mod init;
pub mod publish;
pub mod validate;

use clap::ValueEnum;
use livreur::DiagnosticReport;
use serde::Serialize;

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Serialize)]
pub struct JsonOutput<'a, T: Serialize> {
    pub valid: bool,
    pub schema: Option<u32>,
    pub resolved: Option<&'a T>,
    pub release: Option<&'a livreur::ReleaseConfig>,
    pub warnings: Vec<String>,
    pub errors: Vec<livreur::Diagnostic>,
}

pub fn eprint_report(report: &DiagnosticReport) {
    eprintln!(
        "configuration is invalid ({} error{}):",
        report.errors.len(),
        if report.errors.len() == 1 { "" } else { "s" }
    );
    for error in &report.errors {
        eprintln!("  {}: {}", error.path, error.message);
    }
}

pub fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializable output")
    );
}

pub fn resolve_tag(flag: Option<&str>) -> Result<String, DiagnosticReport> {
    if let Some(tag) = flag.filter(|tag| !tag.is_empty()) {
        return Ok(tag.to_owned());
    }
    if let Some(tag) = std::env::var("GITHUB_REF_NAME")
        .ok()
        .filter(|tag| !tag.is_empty())
    {
        return Ok(tag);
    }
    Err(DiagnosticReport::one(
        "--tag",
        "no tag given; pass --tag or set GITHUB_REF_NAME",
    ))
}
