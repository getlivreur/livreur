use crate::DiagnosticReport;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Computes the lowercase hexadecimal SHA-256 digest of a file.
///
/// # Errors
///
/// Returns diagnostics when the file cannot be opened or read.
pub fn sha256_hex(path: impl AsRef<Path>) -> Result<String, DiagnosticReport> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|error| {
        DiagnosticReport::one(path.display().to_string(), format!("cannot open: {error}"))
    })?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            DiagnosticReport::one(path.display().to_string(), format!("cannot read: {error}"))
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        }))
}

#[must_use]
pub fn sha256sums(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .fold(String::new(), |mut output, (name, digest)| {
            writeln!(output, "{digest}  {name}").expect("writing to a String cannot fail");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn hashes_known_vector_and_formats_sums() {
        let path = std::env::temp_dir().join(format!("livreur-sha-{}", std::process::id()));
        fs::write(&path, b"abc").expect("write fixture");
        let digest = sha256_hex(&path).expect("hash fixture");
        fs::remove_file(path).ok();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256sums(&[("file".into(), digest.clone())]),
            format!("{digest}  file\n")
        );
    }
}
