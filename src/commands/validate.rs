use super::{JsonOutput, OutputFormat, eprint_report, print_json};
use livreur::{Config, validate_release_template};
use std::path::PathBuf;

pub fn validate(
    config: PathBuf,
    manifest: PathBuf,
    tag: Option<&str>,
    format: OutputFormat,
) -> i32 {
    let resolved = match Config::load(config, manifest) {
        Ok(resolved) => resolved,
        Err(report) => {
            match format {
                OutputFormat::Human => eprint_report(&report),
                OutputFormat::Json => print_json(&JsonOutput::<()> {
                    valid: false,
                    schema: None,
                    resolved: None,
                    release: None,
                    warnings: vec![],
                    errors: report.errors,
                }),
            }
            return 2;
        }
    };

    if let Err(report) = validate_release_template(&resolved) {
        match format {
            OutputFormat::Human => eprint_report(&report),
            OutputFormat::Json => print_json(&JsonOutput {
                valid: false,
                schema: Some(resolved.schema),
                resolved: Some(&resolved),
                release: None,
                warnings: vec![],
                errors: report.errors,
            }),
        }
        return 2;
    }

    let release = match tag.map(|tag| resolved.for_tag(tag)).transpose() {
        Ok(release) => release,
        Err(report) => {
            match format {
                OutputFormat::Human => eprint_report(&report),
                OutputFormat::Json => print_json(&JsonOutput {
                    valid: false,
                    schema: Some(resolved.schema),
                    resolved: Some(&resolved),
                    release: None,
                    warnings: vec![],
                    errors: report.errors,
                }),
            }
            return 2;
        }
    };

    match format {
        OutputFormat::Human => println!(
            "configuration is valid: {} {} ({} targets)",
            resolved.package.name,
            resolved.package.version,
            resolved.targets.len()
        ),
        OutputFormat::Json => print_json(&JsonOutput {
            valid: true,
            schema: Some(resolved.schema),
            resolved: Some(&resolved),
            release: release.as_ref(),
            warnings: vec![],
            errors: vec![],
        }),
    }
    0
}
