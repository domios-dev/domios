//! Canonical SHA-256 hash of a plugin bundle.
//!
//! Used by **both** `build.rs` (to generate `SYSTEM_PLUGIN_HASHES`)
//! and the runtime loader (to verify a discovered plugin against
//! that table). One implementation — no drift between build-time
//! and runtime hashes is possible by construction.
//!
//! Algorithm (must produce identical hash on Windows / macOS / Linux):
//!
//!   1. Walk the plugin directory recursively. Skip:
//!      `.DS_Store`, `Thumbs.db`, `desktop.ini`, any path component
//!      starting with `.` (catches `.git/`), `node_modules/`,
//!      `target/`. Empty directories are not hashed.
//!   2. Refuse symlinks — return Err so the build fails loudly.
//!   3. Sort surviving entries by **relative path bytes** (locale
//!      independent).
//!   4. For each file:
//!      - Read raw bytes.
//!      - If extension is in the text set, normalise line endings:
//!        `\r\n` -> `\n`, lone `\r` -> `\n`.
//!      - Otherwise hashed as-is.
//!   5. Per-entry hash = `sha256(path_bytes || 0x00 || content)`,
//!      where path uses forward slashes regardless of OS.
//!   6. Bundle hash = `sha256(concat of per-entry hashes in sort
//!      order)`.
//!   7. File permissions / ownership ignored.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Extensions whose contents are line-ending-normalised before hashing.
const TEXT_EXTENSIONS: &[&str] = &[
    "js", "ts", "tsx", "jsx", "mjs", "cjs", "json", "jsonc", "md", "mdx", "txt", "yml", "yaml",
    "toml", "svg", "html", "css",
];

/// Directory / file names skipped during the walk.
const SKIP_NAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "node_modules",
    "target",
];

#[derive(Debug)]
pub enum HashError {
    Io(io::Error),
    Symlink(PathBuf),
    NotADirectory(PathBuf),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashError::Io(e) => write!(f, "io error: {e}"),
            HashError::Symlink(p) => {
                write!(
                    f,
                    "symlink not allowed inside plugin bundle: {}",
                    p.display()
                )
            }
            HashError::NotADirectory(p) => {
                write!(f, "expected a directory, got: {}", p.display())
            }
        }
    }
}

impl std::error::Error for HashError {}

impl From<io::Error> for HashError {
    fn from(e: io::Error) -> Self {
        HashError::Io(e)
    }
}

/// Compute the canonical SHA-256 of a plugin bundle.
///
/// Returns a lowercase hex string. Order-invariant w.r.t. filesystem
/// iteration order — relies on explicit sort step.
pub fn hash_bundle(plugin_dir: &Path) -> Result<String, HashError> {
    if !plugin_dir.is_dir() {
        return Err(HashError::NotADirectory(plugin_dir.to_path_buf()));
    }

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    collect_entries(plugin_dir, plugin_dir, &mut entries)?;

    // Sort by path bytes for cross-platform determinism. String sort happens to align with bytes for ASCII paths; for non-ASCII names we still
    // want byte-level ordering, not locale ordering.
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut outer = Sha256::new();
    for (rel_path, abs_path) in entries {
        let raw = fs::read(&abs_path)?;
        let content = if is_text(&rel_path) {
            normalise_line_endings(&raw)
        } else {
            raw
        };

        let mut inner = Sha256::new();
        inner.update(rel_path.as_bytes());
        inner.update([0u8]);
        inner.update(&content);
        let digest = inner.finalize();
        outer.update(digest);
    }

    Ok(hex_encode(&outer.finalize()))
}

fn collect_entries(
    root: &Path,
    cur: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), HashError> {
    for entry in fs::read_dir(cur)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip dotfiles + the named-skip list at any depth.
        if name_str.starts_with('.') || SKIP_NAMES.iter().any(|s| *s == name_str) {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            return Err(HashError::Symlink(path));
        }

        if file_type.is_dir() {
            collect_entries(root, &path, out)?;
            continue;
        }

        if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| {
                    HashError::Io(io::Error::other(format!(
                        "path {} not inside root {}",
                        path.display(),
                        root.display()
                    )))
                })?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
        // FIFOs, sockets, etc. are silently ignored (not expected in a plugin bundle).
    }
    Ok(())
}

fn is_text(rel_path: &str) -> bool {
    rel_path
        .rsplit_once('.')
        .map(|(_, ext)| TEXT_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn normalise_line_endings(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b == b'\r' {
            // Skip a following \n (CRLF) or emit \n for lone \r.
            out.push(b'\n');
            if i + 1 < raw.len() && raw[i + 1] == b'\n' {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let abs = dir.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(abs).unwrap();
        f.write_all(content).unwrap();
    }

    #[test]
    fn deterministic_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "plugin.json", b"{\"id\":\"x\"}\n");
        write(dir.path(), "main.js", b"export default 1;\n");
        let h1 = hash_bundle(dir.path()).unwrap();
        let h2 = hash_bundle(dir.path()).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn crlf_lf_equivalent_for_text_files() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "main.js", b"line1\r\nline2\r\n");
        let b = tempfile::tempdir().unwrap();
        write(b.path(), "main.js", b"line1\nline2\n");
        assert_eq!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn lone_cr_normalised() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "main.js", b"line1\rline2\r");
        let b = tempfile::tempdir().unwrap();
        write(b.path(), "main.js", b"line1\nline2\n");
        assert_eq!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn binary_files_not_normalised() {
        // A.png file containing \r\n bytes should NOT be normalised — bumping the count of \r bytes must change the hash.
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "icon.png", b"\x89PNG\r\n\x1a\n");
        let b = tempfile::tempdir().unwrap();
        write(b.path(), "icon.png", b"\x89PNG\n\x1a\n");
        assert_ne!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn dotfiles_and_skips_ignored() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "main.js", b"x\n");

        let b = tempfile::tempdir().unwrap();
        write(b.path(), "main.js", b"x\n");
        write(b.path(), ".DS_Store", b"junk");
        write(b.path(), ".git/HEAD", b"ref: refs/heads/main\n");
        write(b.path(), "node_modules/foo/index.js", b"console.log(1)\n");
        write(b.path(), "target/debug/x", b"compiled");

        assert_eq!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn nested_directory_order_independent() {
        // Filesystem walk may visit "z/a.js" before "a/z.js" depending on FS; explicit sort step must produce the same hash.
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "a/file.js", b"a\n");
        write(a.path(), "z/file.js", b"z\n");

        let b = tempfile::tempdir().unwrap();
        write(b.path(), "z/file.js", b"z\n");
        write(b.path(), "a/file.js", b"a\n");

        assert_eq!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn forward_slash_canonical_path_separator() {
        // The relative path component fed into the inner hash uses '/' not '\'. We verify this implicitly: a bundle hashed on Windows should match one
        // hashed on Unix for identical content. Direct check: rename file to nested and verify hash depends on logical name, not raw OS separator.
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "sub/file.js", b"x\n");
        let h = hash_bundle(a.path()).unwrap();

        // Compute expected hash from canonical form ourselves.
        let mut inner = Sha256::new();
        inner.update(b"sub/file.js");
        inner.update([0u8]);
        inner.update(b"x\n");
        let entry = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(entry);
        assert_eq!(h, hex_encode(&outer.finalize()));
    }

    #[test]
    fn different_content_different_hash() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "main.js", b"a");
        let b = tempfile::tempdir().unwrap();
        write(b.path(), "main.js", b"b");
        assert_ne!(
            hash_bundle(a.path()).unwrap(),
            hash_bundle(b.path()).unwrap()
        );
    }

    #[test]
    fn missing_dir_errors() {
        let path = std::env::temp_dir().join("does-not-exist-codeterm-test");
        let _ = fs::remove_dir_all(&path);
        assert!(matches!(
            hash_bundle(&path),
            Err(HashError::NotADirectory(_))
        ));
    }
}
