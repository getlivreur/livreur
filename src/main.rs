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
    /// Create a livreur.toml populated with the default configuration.
    Init {
        #[arg(long, default_value = "livreur.toml")]
        config: PathBuf,
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
        Command::Init { config } => commands::init::init(&config),
    };
    if code != 0 {
        std::process::exit(code);
    }
}
