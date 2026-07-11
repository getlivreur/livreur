use livreur::DEFAULT_CONFIG_TEMPLATE;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::Path;

pub fn init(config: &Path) -> i32 {
    let mut file = match File::create_new(config) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            eprintln!("{} already exists; refusing to overwrite", config.display());
            return 2;
        }
        Err(e) => {
            eprintln!("cannot write {}: {e}", config.display());
            return 2;
        }
    };
    if let Err(e) = file.write_all(DEFAULT_CONFIG_TEMPLATE.as_bytes()) {
        drop(file);
        eprintln!("cannot write {}: {e}", config.display());
        if let Err(e) = fs::remove_file(config) {
            eprintln!(
                "could not remove partial {}: {e}; delete it before retrying",
                config.display()
            );
        }
        return 2;
    }
    println!("created {}", config.display());
    0
}
