use livreur::{Config, DEFAULT_CONFIG_TEMPLATE, DEFAULT_TARGETS, render_workflow};
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::Path;

pub fn init(config: &Path, workflow: &Path, no_workflow: bool) -> i32 {
    let config_status = write_new(config, DEFAULT_CONFIG_TEMPLATE);
    if config_status == WriteStatus::Existing {
        if no_workflow {
            eprintln!("{} already exists; refusing to overwrite", config.display());
        } else {
            println!("using existing {}", config.display());
        }
    }
    let workflow_status = if no_workflow {
        None
    } else {
        let workflow_config = match config_status {
            WriteStatus::Created | WriteStatus::Existing => {
                match Config::load_workflow_config(config) {
                    Ok(workflow_config) => workflow_config,
                    Err(report) => {
                        eprintln!("cannot load workflow configuration: {report}");
                        return 2;
                    }
                }
            }
            WriteStatus::Failed => livreur::WorkflowConfig {
                tool_version: None,
                crates_enabled: false,
                crates_locked: true,
            },
        };
        let targets = DEFAULT_TARGETS
            .iter()
            .map(|target| (*target).to_owned())
            .collect::<Vec<_>>();
        match render_workflow(
            &targets,
            "v*",
            workflow_config.tool_version.as_ref(),
            workflow_config.crates_enabled,
            workflow_config.crates_locked,
        ) {
            Ok(contents) => {
                let status = write_new(workflow, &contents);
                if status == WriteStatus::Existing {
                    eprintln!(
                        "{} already exists; refusing to overwrite",
                        workflow.display()
                    );
                }
                Some(status)
            }
            Err(report) => {
                eprintln!("cannot render workflow: {report}");
                Some(WriteStatus::Failed)
            }
        }
    };

    let config_ok = config_status == WriteStatus::Created
        || (!no_workflow && config_status == WriteStatus::Existing);
    let workflow_ok = workflow_status.is_none_or(|status| status == WriteStatus::Created);
    if config_ok && workflow_ok { 0 } else { 2 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteStatus {
    Created,
    Existing,
    Failed,
}

fn write_new(path: &Path, contents: &str) -> WriteStatus {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && let Err(e) = fs::create_dir_all(parent)
    {
        eprintln!("cannot create {}: {e}", parent.display());
        return WriteStatus::Failed;
    }
    let mut file = match File::create_new(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return WriteStatus::Existing,
        Err(e) => {
            eprintln!("cannot write {}: {e}", path.display());
            return WriteStatus::Failed;
        }
    };
    if let Err(e) = file.write_all(contents.as_bytes()) {
        drop(file);
        eprintln!("cannot write {}: {e}", path.display());
        if let Err(e) = fs::remove_file(path) {
            eprintln!(
                "could not remove partial {}: {e}; delete it before retrying",
                path.display()
            );
        }
        return WriteStatus::Failed;
    }
    println!("created {}", path.display());
    WriteStatus::Created
}
