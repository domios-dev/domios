use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use super::extract::Lang;

/// Walk supported source files (every extension `Lang::extensions()`
/// covers) under `root`, filtered by `paths` if non-empty. Honors
/// `.gitignore` / `.ignore` via the `ignore` crate's `standard_filters`.
pub fn source_files(root: &Path, paths: &[String]) -> Vec<PathBuf> {
    let exts = Lang::extensions();
    let scan_roots = scan_roots(root, paths);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in scan_roots {
        if !r.exists() {
            continue;
        }
        let mut builder = WalkBuilder::new(&r);
        builder
            .standard_filters(true)
            .hidden(true)
            .follow_links(false)
            .require_git(false);
        for entry in builder.build().flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let ext = match p.extension().and_then(|s| s.to_str()) {
                Some(e) => e,
                None => continue,
            };
            if !exts.iter().any(|e| *e == ext) {
                continue;
            }
            // Skip TypeScript declaration files. They live alongside .ts
            // sources, can be hundreds of KB each (npm types, generated
            // ambient libs), produce thousands of empty interface tags,
            // and dominate ranking output without carrying actual
            // implementation detail.
            if ext == "ts" {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    if name.ends_with(".d.ts") {
                        continue;
                    }
                }
            }
            let canonical = p.to_path_buf();
            if seen.insert(canonical.clone()) {
                out.push(canonical);
            }
        }
    }
    out.sort();
    out
}


fn scan_roots(root: &Path, paths: &[String]) -> Vec<PathBuf> {
    if paths.is_empty() {
        return vec![root.to_path_buf()];
    }
    paths
        .iter()
        .map(|p| {
            let pb = PathBuf::from(p);
            if pb.is_absolute() {
                pb
            } else {
                root.join(p)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_rs_files_at_root() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();
        fs::write(dir.path().join("c.txt"), "ignore me").unwrap();
        let files = source_files(dir.path(), &[]);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().unwrap() == "rs"));
    }

    #[test]
    fn finds_nested_rs_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/sub")).unwrap();
        fs::write(dir.path().join("src/sub/x.rs"), "fn x() {}").unwrap();
        fs::write(dir.path().join("src/y.rs"), "fn y() {}").unwrap();
        let files = source_files(dir.path(), &[]);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn restricts_via_paths() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("api")).unwrap();
        fs::create_dir_all(dir.path().join("db")).unwrap();
        fs::create_dir_all(dir.path().join("ui")).unwrap();
        fs::write(dir.path().join("api/a.rs"), "fn a() {}").unwrap();
        fs::write(dir.path().join("db/b.rs"), "fn b() {}").unwrap();
        fs::write(dir.path().join("ui/c.rs"), "fn c() {}").unwrap();
        let files = source_files(
            dir.path(),
            &["api".to_string(), "db".to_string()],
        );
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| !p.to_string_lossy().contains("ui")));
    }

    #[test]
    fn respects_gitignore() {
        let dir = tempdir().unwrap();
        // ignore needs a `.git` dir or `.gitignore` in a parent — `.gitignore` works
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        fs::create_dir_all(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/x.rs"), "fn x() {}").unwrap();
        fs::write(dir.path().join("ok.rs"), "fn ok() {}").unwrap();
        let files = source_files(dir.path(), &[]);
        let names: Vec<&str> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"ok.rs"));
        assert!(!names.iter().any(|n| *n == "x.rs"));
    }

    #[test]
    fn returns_empty_for_nonexistent_path() {
        let dir = tempdir().unwrap();
        let files = source_files(
            dir.path(),
            &["does_not_exist".to_string()],
        );
        assert!(files.is_empty());
    }

    #[test]
    fn skips_d_ts_declaration_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.ts"), "export const a = 1;").unwrap();
        fs::write(dir.path().join("a.d.ts"), "declare const a: number;").unwrap();
        fs::write(dir.path().join("nested.d.ts"), "// types").unwrap();
        let files = source_files(dir.path(), &[]);
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.ts".to_string()));
        assert!(!names.iter().any(|n| n.ends_with(".d.ts")));
    }

    #[test]
    fn picks_up_typescript_and_javascript_extensions() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.ts"), "export function a() {}").unwrap();
        fs::write(dir.path().join("b.tsx"), "export const B = () => null;").unwrap();
        fs::write(dir.path().join("c.js"), "function c() {}").unwrap();
        fs::write(dir.path().join("d.mjs"), "export function d() {}").unwrap();
        fs::write(dir.path().join("e.go"), "package main").unwrap();
        let files = source_files(dir.path(), &[]);
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.ts".to_string()));
        assert!(names.contains(&"b.tsx".to_string()));
        assert!(names.contains(&"c.js".to_string()));
        assert!(names.contains(&"d.mjs".to_string()));
        assert!(!names.iter().any(|n| n.ends_with(".go")));
    }

    #[test]
    fn output_is_sorted_and_deduped() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("z")).unwrap();
        fs::create_dir_all(dir.path().join("a")).unwrap();
        fs::write(dir.path().join("z/file.rs"), "fn z() {}").unwrap();
        fs::write(dir.path().join("a/file.rs"), "fn a() {}").unwrap();
        let files = source_files(
            dir.path(),
            &["a".to_string(), "z".to_string(), "a".to_string()],
        );
        assert_eq!(files.len(), 2);
        let strs: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(strs[0] < strs[1]);
    }
}
