---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_76766cc849574dbd90441cc63c6da003
handle: persistence-rescan-on-restart
title: Persistence Rescan On Restart
---

**HOW plans survive a daemon restart.** The bug: after restart, `list` only filtered the in-memory map and never scanned disk, and watchers weren't attached to pre-existing project dirs — so plans on disk were invisible until re-seeded. The fix is `PlanStore::scan_project`, which reads the project's `.codeterm/plans` (and `specs`) dir and loads every JSON into the index, skipping malformed files instead of aborting:

```rust
pub fn scan_project(&self, project_path: &Path) -> Result<PathBuf, PlanError> {
    let project_root = paths::project_root(project_path);          // worktree-aware
    let plans_dir = paths::plans_dir(&project_root)?;
    for entry in std::fs::read_dir(&plans_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        if let Err(e) = self.load_file_into_index(path.clone(), &project_root) {
            log::warn!("[plan] skip malformed plan {}: {}", path.display(), e);   // resilient
        }
    }
    // …same loop for specs_dir
    Ok(project_root)
}
```

The wiring that makes restart whole: list-by-project lazily `scan_project`s the dir on first access, startup scans the open-workspace CWDs, and the file-watcher (`watcher.rs`) is attached to every scanned dir so post-restart external edits still flow. Verified by restarting dev and reloading the board with no re-seed — the dogfood plan is just there.

## Key decisions
- **Lazy scan on list-by-project** — a project's plans load when its board is first opened, not eagerly for every project on disk; bounded startup cost.
- **Skip-malformed, don't abort** — one corrupt JSON can't hide a whole project's plans; it's logged and stepped over.
- **Scan + watch in lockstep** — every dir we scan we also watch, so "loaded once at startup" doesn't mean "blind to later edits".
