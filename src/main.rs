mod commands;

use clap::{Parser, Subcommand};
use commands::OutputFormat;
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
    /// Build and upload one target's release archive.
    Build {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
        #[arg(long, default_value = "Cargo.toml")]
        manifest_path: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    /// Verify release assets, upload checksums, and publish the release.
    Publish {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
        #[arg(long, default_value = "Cargo.toml")]
        manifest_path: PathBuf,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    /// Create a livreur.toml populated with the default configuration.
    Init {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".github/workflows/release.yml")]
        workflow: PathBuf,
        #[arg(long)]
        no_workflow: bool,
    },
    /// Extract customizable templates.
    Template {
        #[command(subcommand)]
        command: TemplateCommand,
    },
}

#[derive(Subcommand)]
enum TemplateCommand {
    /// Extract and configure the GitHub release description template.
    Release {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".github/release.md.tera")]
        output: PathBuf,
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let Cli { command } = Cli::parse();
    let code = match command {
        Command::Validate {
            config,
            manifest_path,
            tag,
            format,
        } => commands::validate::validate(config, manifest_path, tag.as_deref(), format),
        Command::Build {
            config,
            manifest_path,
            target,
            tag,
            format,
        } => commands::build::build(&config, &manifest_path, &target, tag.as_deref(), format),
        Command::Publish {
            config,
            manifest_path,
            tag,
            format,
        } => commands::publish::publish(&config, &manifest_path, tag.as_deref(), format),
        Command::Init {
            config,
            workflow,
            no_workflow,
        } => commands::init::init(&config, &workflow, no_workflow),
        Command::Template {
            command:
                TemplateCommand::Release {
                    config,
                    output,
                    force,
                },
        } => commands::template::release(&config, &output, force),
    };
    if code != 0 {
        std::process::exit(code);
    }
}
