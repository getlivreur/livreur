use crate::DiagnosticReport;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufReader, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

#[derive(Clone, Copy)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

pub struct ArchiveEntry {
    pub src: PathBuf,
    pub name: String,
    pub executable: bool,
}

/// Creates a flat release archive from `entries` at `dest`.
///
/// # Errors
///
/// Returns diagnostics when an input cannot be read or the destination archive
/// cannot be created or finalized.
pub fn create_archive(
    kind: ArchiveKind,
    entries: &[ArchiveEntry],
    dest: &Path,
) -> Result<(), DiagnosticReport> {
    match kind {
        ArchiveKind::TarGz => create_tar_gz(entries, dest),
        ArchiveKind::Zip => create_zip(entries, dest),
    }
}

fn create_tar_gz(entries: &[ArchiveEntry], dest: &Path) -> Result<(), DiagnosticReport> {
    let file = File::create(dest).map_err(|e| archive_error(dest, e))?;
    let encoder = GzEncoder::new(BufWriter::new(file), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for entry in entries {
        let file = File::open(&entry.src).map_err(|e| archive_error(&entry.src, e))?;
        let size = file
            .metadata()
            .map_err(|e| archive_error(&entry.src, e))?
            .len();
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_mode(if entry.executable { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, &entry.name, BufReader::new(file))
            .map_err(|e| archive_error(dest, e))?;
    }
    let encoder = builder.into_inner().map_err(|e| archive_error(dest, e))?;
    let mut output = encoder.finish().map_err(|e| archive_error(dest, e))?;
    output.flush().map_err(|e| archive_error(dest, e))?;
    Ok(())
}

fn create_zip(entries: &[ArchiveEntry], dest: &Path) -> Result<(), DiagnosticReport> {
    let file = File::create(dest).map_err(|e| archive_error(dest, e))?;
    write_zip(BufWriter::new(file), entries, dest)
}

fn write_zip<W: Write + Seek>(
    output: W,
    entries: &[ArchiveEntry],
    dest: &Path,
) -> Result<(), DiagnosticReport> {
    let mut writer = zip::ZipWriter::new(output);
    for entry in entries {
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(if entry.executable { 0o755 } else { 0o644 });
        writer
            .start_file(&entry.name, options)
            .map_err(|e| archive_error(dest, e))?;
        let mut file = File::open(&entry.src).map_err(|e| archive_error(&entry.src, e))?;
        std::io::copy(&mut file, &mut writer).map_err(|e| archive_error(dest, e))?;
    }
    let mut output = writer.finish().map_err(|e| archive_error(dest, e))?;
    output.flush().map_err(|e| archive_error(dest, e))?;
    Ok(())
}

fn archive_error(path: &Path, error: impl std::fmt::Display) -> DiagnosticReport {
    DiagnosticReport::one(
        path.display().to_string(),
        format!("archive error: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Cursor, Error, Read, SeekFrom};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("livreur-archive-{}-{nonce}", std::process::id()));
            fs::create_dir(&path).expect("create fixture");
            Self(path)
        }

        fn entries(&self) -> Vec<ArchiveEntry> {
            let binary = self.0.join("source-binary");
            let readme = self.0.join("source-readme");
            fs::write(&binary, b"binary contents").expect("write binary");
            fs::write(&readme, b"documentation").expect("write readme");
            vec![
                ArchiveEntry {
                    src: binary,
                    name: "tool".into(),
                    executable: true,
                },
                ArchiveEntry {
                    src: readme,
                    name: "README.md".into(),
                    executable: false,
                },
            ]
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn tar_gz_is_flat_and_preserves_modes_and_contents() {
        let fixture = Fixture::new();
        let archive_path = fixture.0.join("asset.tar.gz");
        create_archive(ArchiveKind::TarGz, &fixture.entries(), &archive_path)
            .expect("create archive");

        let decoder = flate2::read::GzDecoder::new(File::open(archive_path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let mut found = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().into_owned();
            let mode = entry.header().mode().unwrap();
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            found.push((name, mode, contents));
        }
        assert_eq!(
            found[0],
            (PathBuf::from("tool"), 0o755, "binary contents".into())
        );
        assert_eq!(
            found[1],
            (PathBuf::from("README.md"), 0o644, "documentation".into())
        );
    }

    #[test]
    fn zip_is_flat_and_preserves_modes_and_contents() {
        let fixture = Fixture::new();
        let archive_path = fixture.0.join("asset.zip");
        create_archive(ArchiveKind::Zip, &fixture.entries(), &archive_path)
            .expect("create archive");

        let mut archive = zip::ZipArchive::new(File::open(archive_path).unwrap()).unwrap();
        let mut binary = archive.by_name("tool").unwrap();
        assert_eq!(binary.unix_mode(), Some(0o100755));
        let mut contents = String::new();
        binary.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "binary contents");
        drop(binary);
        let mut readme = archive.by_name("README.md").unwrap();
        assert_eq!(readme.unix_mode(), Some(0o100644));
        contents.clear();
        readme.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "documentation");
    }

    #[test]
    fn zip_propagates_the_final_writer_flush_error() {
        let fixture = Fixture::new();
        let writer = BufWriter::new(FlushErrorWriter(Cursor::new(Vec::new())));

        let error = write_zip(writer, &fixture.entries(), Path::new("asset.zip"))
            .expect_err("flush should fail");

        assert!(error.to_string().contains("final flush failed"));
    }

    struct FlushErrorWriter(Cursor<Vec<u8>>);

    impl Write for FlushErrorWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(Error::other("final flush failed"))
        }
    }

    impl Seek for FlushErrorWriter {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(position)
        }
    }
}
