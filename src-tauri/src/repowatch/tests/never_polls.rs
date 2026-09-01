use std::path::{Path, PathBuf};

/// R7 — structural, green before and after: the deleted poll cannot return
/// under a new name anywhere in the repository-watch path.
#[test]
fn repository_watch_never_polls() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = collect(&manifest.join("src").join("repowatch"));
    sources.push(manifest.join("src").join("commands").join("repo_watch.rs"));
    assert!(
        sources.len() > 5,
        "expected the repository-watch sources, found {}",
        sources.len()
    );

    let banned = [
        "PollWatcher",
        "with_poll_interval",
        "poll_interval",
        "thread::sleep",
        "tokio::time::sleep",
    ];
    for source in &sources {
        let text = std::fs::read_to_string(source)
            .unwrap_or_else(|e| panic!("read {}: {e}", source.display()));
        for needle in banned {
            assert!(
                !text.contains(needle),
                "{} must not contain {needle}",
                source.display()
            );
        }
    }

    // notify's Windows and Linux backends are unconditional per-target deps;
    // only macOS FSEvents needs a feature, and losing it falls back to kqueue.
    guard_oracles_never_wait_on_time(&manifest);

    let cargo = std::fs::read_to_string(manifest.join("Cargo.toml")).expect("read Cargo.toml");
    let notify_line = cargo
        .lines()
        .find(|line| line.starts_with("notify ="))
        .expect("notify dependency entry");
    assert!(
        notify_line.contains("macos_fsevent"),
        "notify must keep macos_fsevent enabled, got: {notify_line}"
    );
}

fn collect(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // The oracle's own file names what it bans; scan production only.
            if path.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            found.extend(collect(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found
}

/// The oracles must never coordinate on elapsed time. Only `support.rs` holds
/// the sanctioned channel waits; this file is skipped because it names them.
fn guard_oracles_never_wait_on_time(manifest: &Path) {
    let dir = manifest.join("src").join("repowatch").join("tests");
    let entries = std::fs::read_dir(&dir).expect("repository-watch tests directory");
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if !name.ends_with(".rs") || name == "never_polls.rs" {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(&path).expect("read oracle source");
        for needle in ["settle", "sleep", "quiesce"] {
            assert!(!text.contains(needle), "{name} must not contain {needle}");
        }
        if name != "support.rs" {
            assert!(
                !text.contains("recv_timeout"),
                "{name} must wait through support.rs, not on its own timed recv"
            );
        }
    }
    assert!(checked >= 8, "expected the oracle sources, found {checked}");
}
