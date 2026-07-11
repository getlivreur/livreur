use clap::{Parser, Subcommand, ValueEnum};
use livreur::{Config, DiagnosticReport};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "livreur", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate livreur.toml and its Cargo package without side effects.
    Validate {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
        #[arg(long, default_value = "Cargo.toml")]
        manifest_path: PathBuf,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Serialize)]
struct JsonOutput<'a, T: Serialize> {
    valid: bool,
    schema: Option<u32>,
    resolved: Option<&'a T>,
    release: Option<&'a livreur::ReleaseConfig>,
    warnings: Vec<String>,
    errors: Vec<livreur::Diagnostic>,
}

fn main() {
    let Cli { command } = Cli::parse();
    let code = match command {
        Command::Validate {
            config,
            manifest_path,
            tag,
            format,
        } => validate(config, manifest_path, tag.as_deref(), format),
    };
    if code != 0 {
        std::process::exit(code);
    }
}

fn validate(config: PathBuf, manifest: PathBuf, tag: Option<&str>, format: OutputFormat) -> i32 {
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

fn eprint_report(report: &DiagnosticReport) {
    eprintln!(
        "configuration is invalid ({} error{}):",
        report.errors.len(),
        if report.errors.len() == 1 { "" } else { "s" }
    );
    for error in &report.errors {
        eprintln!("  {}: {}", error.path, error.message);
    }
}
fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializable output")
    );
}
