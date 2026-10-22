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

    let packaged = runtime_file_rules(plugin_dir)?;
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    collect_entries(plugin_dir, plugin_dir, &mut entries)?;

    entries.retain(|(relative, _)| {
        packaged
            .as_ref()
            .is_none_or(|rules| runtime_file_allowed(relative, rules))
    });
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
mod tests;

#[path = "../../codeterm-util/src/plugin_runtime_files.rs"]
mod runtime_files;
pub use runtime_files::copy_runtime_bundle;
use runtime_files::{runtime_file_allowed, runtime_file_rules};
