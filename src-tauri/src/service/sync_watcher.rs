//! Reactive filesystem watcher for cross-provider sync.
//!
//! Watches:
//! - `~/.codeterm/skills/` → mirrors to `~/.claude/skills/` and `~/.agents/skills/`
//! - `~/.codeterm/general/MEMORY.md` → regenerates `AGENTS.md` (Codex needs inline content)
//!
//! Uses `notify` crate with debouncing. Runs on a dedicated std::thread.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

/// Provider skill directories that we mirror into.
fn provider_skill_dirs() -> Vec<PathBuf> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    vec![
        home.join(".claude").join("skills"),
        home.join(".agents").join("skills"),
    ]
}

/// Start the reactive sync watcher on a background thread.
/// Watches `{codeterm_dir}/skills/` and `{codeterm_dir}/general/MEMORY.md`.
pub fn start(codeterm_dir: PathBuf) {
    let skills_dir = codeterm_dir.join("skills");
    let general_dir = codeterm_dir.join("general");
    let memory_path = general_dir.join("MEMORY.md");

    // Initial sync on startup
    if skills_dir.exists() {
        sync_skills(&skills_dir);
    }

    std::thread::Builder::new()
        .name("sync-watcher".into())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut debouncer = match new_debouncer(Duration::from_millis(300), tx) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("[sync-watcher] Failed to create debouncer: {}", e);
                    return;
                }
            };

            // Watch skills directory
            let _ = std::fs::create_dir_all(&skills_dir);
            if let Err(e) = debouncer.watcher().watch(&skills_dir, notify::RecursiveMode::Recursive) {
                log::error!("[sync-watcher] Failed to watch skills dir: {}", e);
            } else {
                log::info!("[sync-watcher] Watching {}", skills_dir.display());
            }

            // Watch general directory (for MEMORY.md changes)
            if general_dir.exists() {
                if let Err(e) = debouncer.watcher().watch(&general_dir, notify::RecursiveMode::NonRecursive) {
                    log::error!("[sync-watcher] Failed to watch general dir: {}", e);
                } else {
                    log::info!("[sync-watcher] Watching {}", general_dir.display());
                }
            }

            loop {
                match rx.recv() {
                    Ok(Ok(events)) => {
                        let mut skills_changed = false;
                        let mut memory_changed = false;

                        for event in &events {
                            if event.kind == DebouncedEventKind::Any {
                                if event.path.starts_with(&skills_dir) {
                                    skills_changed = true;
                                } else if event.path == memory_path {
                                    memory_changed = true;
                                }
                            }
                        }

                        if skills_changed {
                            log::info!("[sync-watcher] Skills changed, syncing to providers");
                            sync_skills(&skills_dir);
                        }
                        if memory_changed {
                            log::info!("[sync-watcher] MEMORY.md changed, regenerating AGENTS.md");
                            regenerate_agents_md(&general_dir);
                        }
                    }
                    Ok(Err(errors)) => {
                        log::warn!("[sync-watcher] Watch error: {:?}", errors);
                    }
                    Err(_) => {
                        log::info!("[sync-watcher] Channel closed, stopping");
                        break;
                    }
                }
            }
        })
        .expect("Failed to spawn sync-watcher thread");
}

/// Mirror `~/.codeterm/skills/` → provider skill directories.
/// Full mirror: creates, updates, and removes stale entries.
fn sync_skills(source: &Path) {
    let targets = provider_skill_dirs();
    for target in &targets {
        if let Err(e) = mirror_dir(source, target) {
            log::warn!("[sync-watcher] Failed to sync skills to {}: {}", target.display(), e);
        }
    }
}

/// Regenerate AGENTS.md with inlined AGENT.md + MEMORY.md + codeterm rule.
/// Called reactively when MEMORY.md changes.
fn regenerate_agents_md(general_dir: &Path) {
    let agent_content = std::fs::read_to_string(general_dir.join("AGENT.md")).unwrap_or_default();
    let memory_content = std::fs::read_to_string(general_dir.join("MEMORY.md")).unwrap_or_default();

    let agents_content = format!(
        "{}\n\n---\n\n{}\n\n---\n\n{}",
        agent_content,
        memory_content,
        crate::prompts::CODETERM_RULE,
    );

    if let Err(e) = std::fs::write(general_dir.join("AGENTS.md"), agents_content) {
        log::error!("[sync-watcher] Failed to write AGENTS.md: {}", e);
    }
}

/// Mirror source directory to target, creating/updating/deleting as needed.
fn mirror_dir(source: &Path, target: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(target);

    // Copy/update from source
    copy_recursive(source, target)?;

    // Remove entries in target that don't exist in source
    clean_stale(source, target)?;

    Ok(())
}

fn copy_recursive(source: &Path, target: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(source).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = target.join(entry.file_name());

        if src.is_dir() {
            let _ = std::fs::create_dir_all(&dst);
            copy_recursive(&src, &dst)?;
        } else if src.is_file() {
            // Only copy if content differs (avoid unnecessary writes that trigger more events)
            let should_copy = if dst.exists() {
                let src_meta = std::fs::metadata(&src).ok();
                let dst_meta = std::fs::metadata(&dst).ok();
                match (src_meta, dst_meta) {
                    (Some(s), Some(d)) => s.len() != d.len() || {
                        // Size matches — check modification time
                        let s_mod = s.modified().ok();
                        let d_mod = d.modified().ok();
                        match (s_mod, d_mod) {
                            (Some(s), Some(d)) => s > d,
                            _ => true,
                        }
                    },
                    _ => true,
                }
            } else {
                true
            };

            if should_copy {
                let _ = std::fs::create_dir_all(dst.parent().unwrap_or(target));
                std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn clean_stale(source: &Path, target: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(target) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let dst = entry.path();
        let src = source.join(entry.file_name());
        if !src.exists() {
            if dst.is_dir() {
                let _ = std::fs::remove_dir_all(&dst);
            } else {
                let _ = std::fs::remove_file(&dst);
            }
        } else if dst.is_dir() && src.is_dir() {
            clean_stale(&src, &dst)?;
        }
    }
    Ok(())
}

/// Public helper: sync skills from canonical dir to all providers (called on initial install).
pub fn sync_skills_now() {
    let codeterm_dir = crate::paths::data_dir();
    let skills_dir = codeterm_dir.join("skills");
    if skills_dir.exists() {
        sync_skills(&skills_dir);
    }
}

/// Public helper: get the canonical skills directory.
pub fn canonical_skills_dir() -> PathBuf {
    crate::paths::data_dir().join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_mirror_dir_copies_files() {
        let tmp = std::env::temp_dir().join("codeterm_sync_test_mirror");
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        fs::create_dir_all(src.join("skill-a")).unwrap();
        fs::write(src.join("skill-a").join("SKILL.md"), "hello").unwrap();

        mirror_dir(&src, &dst).unwrap();

        assert!(dst.join("skill-a").join("SKILL.md").exists());
        assert_eq!(fs::read_to_string(dst.join("skill-a").join("SKILL.md")).unwrap(), "hello");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_mirror_dir_removes_stale() {
        let tmp = std::env::temp_dir().join("codeterm_sync_test_stale");
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dst.join("old-skill")).unwrap();
        fs::write(dst.join("old-skill").join("SKILL.md"), "old").unwrap();

        mirror_dir(&src, &dst).unwrap();

        assert!(!dst.join("old-skill").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_provider_skill_dirs_not_empty() {
        let dirs = provider_skill_dirs();
        // Should have at least Claude and Codex dirs (unless no home dir)
        if dirs::home_dir().is_some() {
            assert_eq!(dirs.len(), 2);
            assert!(dirs[0].to_string_lossy().contains(".claude"));
            assert!(dirs[1].to_string_lossy().contains(".agents"));
        }
    }
}
