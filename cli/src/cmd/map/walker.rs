use crate::cmd::map::lang::Lang;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub lang: Lang,
}

pub fn walk_workspace(root: &Path) -> Vec<WorkspaceFile> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .standard_filters(true)
        .build();

    let mut out = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let abs = entry.path();
        if is_excluded(abs, root) {
            continue;
        }
        let Some(lang) = Lang::from_path(abs) else { continue };
        let Some(rel) = relative(abs, root) else { continue };
        out.push(WorkspaceFile { abs_path: abs.to_path_buf(), rel_path: rel, lang });
    }
    out
}

pub fn relative(abs: &Path, root: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    Some(to_forward_slashes(&rel.to_string_lossy()))
}

fn to_forward_slashes(s: &str) -> String {
    s.replace('\\', "/")
}

fn is_excluded(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return true };
    let s = rel.to_string_lossy();
    let lower = s.to_ascii_lowercase();
    const SKIP_DIRS: &[&str] = &[
        ".codeterm/", "target/", "node_modules/", "dist/", "build/",
        ".git/", ".venv/", "venv/", "__pycache__/", ".next/", ".turbo/",
        ".idea/", ".vscode/", "vendor/", "references/",
    ];
    SKIP_DIRS.iter().any(|d| lower.starts_with(d) || lower.contains(&format!("/{}", d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_target_and_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn x() {}").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/junk.rs"), "fn y() {}").unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("node_modules/x/m.js"), "function z(){}").unwrap();

        let files = walk_workspace(root);
        let rels: Vec<String> = files.iter().map(|f| f.rel_path.clone()).collect();
        assert!(rels.contains(&"src/lib.rs".to_string()), "want lib.rs got {rels:?}");
        assert!(!rels.iter().any(|r| r.starts_with("target/")), "target should be excluded: {rels:?}");
        assert!(!rels.iter().any(|r| r.starts_with("node_modules/")), "node_modules should be excluded: {rels:?}");
    }

    #[test]
    fn forward_slashes_only() {
        let s = to_forward_slashes("a\\b\\c.rs");
        assert_eq!(s, "a/b/c.rs");
    }
}
