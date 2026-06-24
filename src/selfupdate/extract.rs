//! Extract the `gproxy` executable from a downloaded release `.zip` (§19.5) —
//! NATIVE only.
//!
//! The release packager ships each platform as a `.zip` (binary + README). The
//! self-update artifact `url`/`sha256` therefore point at the `.zip`; after the
//! zip's bytes are sha256-checked and the manifest signature is verified, the
//! executable is pulled out here and handed to [`super::swap::install`]. The
//! extracted binary inherits the zip's verified trust — no separate inner hash.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use super::UpdateError;

/// Extract the `gproxy` / `gproxy.exe` entry from `zip_path` to a sibling
/// `<stem>.bin` file and return its path. The zip is expected to be the
/// already-verified release artifact.
pub fn extract_binary(zip_path: &Path) -> Result<PathBuf, UpdateError> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| UpdateError::Integrity(format!("update artifact is not a valid zip: {e}")))?;

    let idx = binary_index(&mut archive).ok_or_else(|| {
        UpdateError::Integrity("update artifact zip contains no gproxy executable".to_string())
    })?;

    let mut entry = archive
        .by_index(idx)
        .map_err(|e| UpdateError::Integrity(format!("reading gproxy from zip: {e}")))?;

    let out = zip_path.with_extension("bin");
    let mut out_file = File::create(&out)?;
    io::copy(&mut entry, &mut out_file)?;
    Ok(out)
}

/// Index of the first entry whose file name is `gproxy` or `gproxy.exe`.
/// `enclosed_name` guards against path-traversal entries.
fn binary_index(archive: &mut ZipArchive<File>) -> Option<usize> {
    (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .ok()
            .and_then(|e| {
                e.enclosed_name()
                    .and_then(|p| p.file_name().map(|f| f.to_owned()))
            })
            .is_some_and(|f| f == "gproxy" || f == "gproxy.exe")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        for (name, data) in entries {
            zw.start_file(*name, SimpleFileOptions::default()).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn extracts_gproxy_ignoring_readme() {
        let dir = std::env::temp_dir().join("gproxy-extract-test");
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("artifact.tmp");
        write_zip(
            &zip_path,
            &[("README.md", b"hello"), ("gproxy", b"\x7fELF-binary-bytes")],
        );

        let out = extract_binary(&zip_path).expect("extract");
        assert_eq!(out, zip_path.with_extension("bin"));
        assert_eq!(std::fs::read(&out).unwrap(), b"\x7fELF-binary-bytes");

        let _ = std::fs::remove_file(&zip_path);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn missing_binary_is_integrity_error() {
        let dir = std::env::temp_dir().join("gproxy-extract-test");
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("no-bin.tmp");
        write_zip(&zip_path, &[("README.md", b"only docs")]);

        let err = extract_binary(&zip_path).unwrap_err();
        assert!(matches!(err, UpdateError::Integrity(_)));
        let _ = std::fs::remove_file(&zip_path);
    }
}
