use livreur::{DEFAULT_CONFIG_TEMPLATE, DEFAULT_TARGETS, render_workflow};
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::Path;

pub fn init(config: &Path, workflow: &Path, no_workflow: bool) -> i32 {
    let config_status = write_new(config, DEFAULT_CONFIG_TEMPLATE);
    let workflow_status = if no_workflow {
        0
    } else {
        let targets = DEFAULT_TARGETS
            .iter()
            .map(|target| (*target).to_owned())
            .collect::<Vec<_>>();
        match render_workflow(&targets, "v*") {
            Ok(contents) => write_new(workflow, &contents),
            Err(report) => {
                eprintln!("cannot render workflow: {report}");
                2
            }
        }
    };
    if config_status == 0 && workflow_status == 0 {
        0
    } else {
        2
    }
}

fn write_new(path: &Path, contents: &str) -> i32 {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && let Err(e) = fs::create_dir_all(parent)
    {
        eprintln!("cannot create {}: {e}", parent.display());
        return 2;
    }
    let mut file = match File::create_new(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            eprintln!("{} already exists; refusing to overwrite", path.display());
            return 2;
        }
        Err(e) => {
            eprintln!("cannot write {}: {e}", path.display());
            return 2;
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
        return 2;
    }
    println!("created {}", path.display());
    0
}
