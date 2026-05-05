pub mod budget;
pub mod cache;
pub mod extract;
pub mod render;
pub mod walk;

use std::path::{Path, PathBuf};

use crate::args::map::MapArgs;
use crate::data_dir_name;

use budget::{rank, score_no_query, score_query, truncate_to_budget, ScoredTag};
use cache::{default_cache_path, mtime_epoch, Cache};
use extract::{extract_rust, Tag};

pub fn handle_map(args: MapArgs) {
    let cwd = match std::env::current_dir() {
        Ok(p) => canonicalize_or_self(&p),
        Err(e) => {
            eprintln!("codeterm map: cannot read current dir: {}", e);
            std::process::exit(2);
        }
    };

    if let Some(ref single) = args.signatures {
        let path = resolve_relative(&cwd, single);
        match dump_single_file(&path) {
            Ok(text) => {
                print!("{}", text);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("codeterm map: {}", e);
                std::process::exit(2);
            }
        }
    }

    let cache_path = default_cache_path(data_dir_name());
    let cache = Cache::open(&cache_path).ok();

    let workspace_key = cwd.to_string_lossy().to_string();
    let scored = scan_workspace(&cwd, &workspace_key, &args, cache.as_ref());
    let mut scored_owned = scored;
    rank(&mut scored_owned);
    let trimmed = truncate_to_budget(&scored_owned, args.max_tokens);
    let rendered = render::render(&trimmed);
    print!("{}", rendered);
    std::process::exit(0);
}

pub fn scan_workspace(
    root: &Path,
    workspace_key: &str,
    args: &MapArgs,
    cache: Option<&Cache>,
) -> Vec<ScoredTag> {
    let files = walk::rust_files(root, &args.paths);
    let query = args.query.clone().unwrap_or_default();
    let mut scored: Vec<ScoredTag> = Vec::new();
    for path in files {
        let rel = relativize(root, &path);
        let mtime = mtime_epoch(&path).unwrap_or(0);
        let tags = load_tags(&path, workspace_key, &rel, mtime, args.refresh, cache);
        for tag in tags {
            let score = if query.trim().is_empty() {
                score_no_query(&tag.name)
            } else {
                let s = score_query(&tag.name, &query);
                if s == 0 {
                    continue;
                }
                s
            };
            scored.push(ScoredTag {
                file: rel.clone(),
                tag,
                score,
            });
        }
    }
    scored
}

fn load_tags(
    path: &Path,
    workspace_key: &str,
    rel: &str,
    mtime: i64,
    refresh: bool,
    cache: Option<&Cache>,
) -> Vec<Tag> {
    if !refresh {
        if let Some(c) = cache {
            if let Some(cached) = c.get(workspace_key, rel, mtime) {
                return cached;
            }
        }
    }
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let tags = extract_rust(&source);
    if let Some(c) = cache {
        let _ = c.put(workspace_key, rel, mtime, &tags);
    }
    tags
}

pub fn dump_single_file(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let tags = extract_rust(&source);
    let display = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let mut out = String::new();
    out.push_str("## ");
    out.push_str(&display);
    out.push('\n');
    let mut sorted = tags;
    sorted.sort_by_key(|t| t.line);
    for t in sorted {
        out.push_str(&display);
        out.push(':');
        out.push_str(&t.line.to_string());
        out.push_str(" | ");
        out.push_str(&t.signature);
        out.push('\n');
    }
    Ok(out)
}

fn relativize(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(p) => p.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

fn canonicalize_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn resolve_relative(root: &Path, p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() {
        pb
    } else {
        root.join(pb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fixture_root() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/repo-map-rust");
        p
    }

    fn args_default() -> MapArgs {
        MapArgs {
            query: None,
            paths: Vec::new(),
            max_tokens: 4000,
            signatures: None,
            refresh: false,
        }
    }

    #[test]
    fn fixture_dir_exists() {
        assert!(fixture_root().is_dir(), "fixture must be checked in");
    }

    #[test]
    fn scan_fixture_finds_expected_tags() {
        let root = fixture_root();
        let scored = scan_workspace(&root, "fixture", &args_default(), None);
        assert!(scored.len() >= 10, "got only {} tags", scored.len());
        let kinds: Vec<&str> = scored.iter().map(|s| s.tag.kind.as_str()).collect();
        assert!(kinds.contains(&"fn"));
        assert!(kinds.contains(&"struct"));
        assert!(kinds.contains(&"enum"));
        assert!(kinds.contains(&"trait"));
        assert!(kinds.contains(&"impl"));
        assert!(kinds.contains(&"mod"));
    }

    #[test]
    fn query_filters_to_matching_subset() {
        let root = fixture_root();
        let mut args = args_default();
        args.query = Some("user".into());
        let scored = scan_workspace(&root, "fixture", &args, None);
        assert!(!scored.is_empty(), "expected matches for 'user'");
        for s in &scored {
            assert!(
                s.tag.name.to_lowercase().contains("user"),
                "unexpected {} in user-query result",
                s.tag.name
            );
        }
    }

    #[test]
    fn query_matches_nothing_returns_empty() {
        let root = fixture_root();
        let mut args = args_default();
        args.query = Some("zzz_no_such_symbol".into());
        let scored = scan_workspace(&root, "fixture", &args, None);
        assert!(scored.is_empty());
    }

    #[test]
    fn max_tokens_truncates_output() {
        let root = fixture_root();
        let mut args = args_default();
        args.max_tokens = 50;
        let scored = scan_workspace(&root, "fixture", &args, None);
        let mut sc = scored;
        rank(&mut sc);
        let truncated = truncate_to_budget(&sc, 50);
        let rendered = render::render(&truncated);
        let est = budget::estimate_tokens(rendered.len());
        assert!(
            est <= 60,
            "tokens={}; rendered={:?}",
            est,
            rendered
        );
        assert!(truncated.len() < sc.len());
    }

    #[test]
    fn signatures_dumps_only_one_file() {
        let root = fixture_root();
        let p = root.join("auth.rs");
        let out = dump_single_file(&p).unwrap();
        assert!(out.contains("authenticate_user") || out.contains("login"));
        assert!(!out.contains("router")); // router lives in router.rs not auth.rs
    }

    #[test]
    fn cache_hit_returns_stored_tags_without_reextract() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.rs");
        fs::write(&path, "pub fn alpha() {}\npub fn beta() {}\n").unwrap();
        let cache = Cache::open_in_memory().unwrap();
        let args = args_default();
        let key = dir.path().to_string_lossy().to_string();

        let first = scan_workspace(dir.path(), &key, &args, Some(&cache));
        assert_eq!(first.len(), 2);

        let mtime = mtime_epoch(&path).unwrap();
        let rel = "a.rs";
        let sentinel = vec![Tag {
            name: "from_cache_only".into(),
            kind: "fn".into(),
            line: 99,
            signature: "fn from_cache_only()".into(),
        }];
        cache.put(&key, rel, mtime, &sentinel).unwrap();

        let second = scan_workspace(dir.path(), &key, &args, Some(&cache));
        assert_eq!(second.len(), 1, "cache must short-circuit re-extract");
        assert_eq!(second[0].tag.name, "from_cache_only");
    }

    #[test]
    fn refresh_skips_cache_and_reextracts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.rs");
        fs::write(&path, "pub fn alpha() {}\npub fn beta() {}\n").unwrap();
        let cache = Cache::open_in_memory().unwrap();
        let mut args = args_default();
        let key = dir.path().to_string_lossy().to_string();

        let mtime = mtime_epoch(&path).unwrap();
        let sentinel = vec![Tag {
            name: "stale_cache_entry".into(),
            kind: "fn".into(),
            line: 1,
            signature: "fn stale_cache_entry()".into(),
        }];
        cache.put(&key, "a.rs", mtime, &sentinel).unwrap();

        args.refresh = true;
        let scored = scan_workspace(dir.path(), &key, &args, Some(&cache));
        assert_eq!(scored.len(), 2);
        let names: Vec<&str> = scored.iter().map(|s| s.tag.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(!names.contains(&"stale_cache_entry"));
    }

    #[test]
    fn paths_filter_restricts_scan() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("api")).unwrap();
        fs::create_dir_all(dir.path().join("db")).unwrap();
        fs::write(dir.path().join("api/a.rs"), "pub fn api_fn() {}").unwrap();
        fs::write(dir.path().join("db/b.rs"), "pub fn db_fn() {}").unwrap();

        let mut args = args_default();
        args.paths = vec!["api".to_string()];
        let key = dir.path().to_string_lossy().to_string();
        let scored = scan_workspace(dir.path(), &key, &args, None);
        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].tag.name, "api_fn");
    }

    #[test]
    fn relativize_uses_forward_slash() {
        let root = Path::new("/abs/root");
        let path = Path::new("/abs/root/sub/file.rs");
        assert_eq!(relativize(root, path), "sub/file.rs");
    }

    #[test]
    fn dump_single_file_missing_path_errors() {
        let p = PathBuf::from("/nonexistent/zzz.rs");
        assert!(dump_single_file(&p).is_err());
    }
}
