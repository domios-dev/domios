**HOW the plan store survives restart, isolates per workspace, and stays the single writer.** The spec says source of truth = per-project JSON, daemon = single writer, identity stable across restart, bodies externalized to editable `.md`. This is the implementation of that contract in `src-tauri/src/service/plan/`.

## 1. Single writer, atomic, body-stripped

Every mutation funnels through one `write_atomic`. The plan is normalized, its large markdown bodies are persisted to sidecar `.md` files and **stripped** out of the JSON, then the JSON is written to a temp file and `rename`d into place — `rename` is atomic on POSIX/NTFS, so a reader never sees a half-written plan and a crash mid-write can't corrupt the store:

```rust
fn write_atomic(plan: &Plan) -> std::io::Result<PathBuf> {
    let path = Self::file_path(plan)?;
    let tmp = path.with_extension("json.tmp");
    let mut normalized = plan.clone();
    tag_model::normalize_plan(&mut normalized);
    body_files::persist_bodies(&normalized)?;        // bodies → sections/*.md, tasks/*.md …
    let json_plan = body_files::strip_bodies(&normalized);
    let json = serde_json::to_string_pretty(&json_plan)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;                    // atomic publish
    Ok(path)
}
```

Specs go through a parallel `write_spec_atomic`. The CLI/REST never touch the files; they call into this in-process function — which is also why a daemon must **never shell out to its own `codeterm` CLI** (architecture smell + slow).

## 2. Externalized bodies: editable on disk, daemon-owned

Bodies over a threshold are spilled to `<plan>/sections/<handle>.md`, `tasks/<handle>.md`, etc., so a human (or this very task) can read/diff them as real markdown. The path is keyed off the section's *handle*, not its id, so files are human-navigable:

```rust
const MARKDOWN_SIDECAR_MIN_CHARS: usize = 240;

pub fn section_path(plan: &Plan, section_id: &str) -> std::io::Result<PathBuf> {
    let handle = flatten_sections(&plan.sections)
        .into_iter()
        .find(|s| s.id == section_id)
        .map(super::tag_model::section_handle)
        .unwrap_or(section_id);
    body_path(plan, BodyKind::Section, handle)
}
```

The daemon stays the single writer even though the files look hand-editable — the CLI handle (`plan section update … --body-file`) is the supported edit path; a raw hand-edit can be clobbered on the next mutation.

## 3. File-watcher with an own-write guard (no reload loops)

A `notify` debouncer (200ms) watches the plans dir so an external edit (git checkout, another worktree) reloads into the in-memory index and emits `plan-changed` to every UI. The trap is feedback: our *own* `write_atomic` fires the watcher. So every write records its path+mtime, and the watcher skips paths whose mtime matches the recorded one:

```rust
fn handle_events(events, store, app) {
    for event in events.into_iter().flatten() {
        let path = event.path;
        if store.is_own_recent_write(&path) { continue; }   // skip our own writes
        let Some(plan_id) = plan_id_from_path(&path) else { continue; };
        let loaded = store.reload_from_disk(&plan_id)
            .or_else(|_| /* fall back to a full project scan */ );
        if loaded.is_ok() {
            crate::api::events::emit_everywhere(app, "plan-changed",
                json!({ "plan_id": plan_id }));
        }
    }
}
```

`is_own_recent_write` compares the on-disk `modified()` time to the time we stamped at write — and `note_plan_write` records not just the JSON but every externalized body path, so editing a body via the CLI doesn't echo back as an "external" change.

## 4. Workspace isolation via worktree-aware project root

"Scoped per workspace" + "stable across restart" means resolving the right `.codeterm` for the cwd. A linked worktree that committed its own plans on its branch owns its store; otherwise we fall back to the main worktree. Without this, a feature-branch worktree's committed plans are invisible (resolution jumps to main, a different branch):

```rust
pub fn project_root(cwd: &Path) -> PathBuf {
    if let Some(top) = worktree_top(cwd) {
        if top.join(".codeterm").is_dir() { return top; }   // worktree owns its store
    }
    if let Some(root) = main_worktree_root(cwd) { return root; }
    if let Some(top) = worktree_top(cwd) { return top; }
    cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
}
```

## Key decisions / tradeoffs
- **temp-write + rename over in-place write** — atomic publish is free durability; the cost is a transient `.json.tmp` the watcher must ignore.
- **Body sidecars over fat JSON** — keeps the JSON diffable and lets bodies be read/edited as markdown; the cost is the strip/persist round-trip on every write and the own-write bookkeeping covering body paths too.
- **Hot-path discipline** — snapshot-style reads that fire on tab-switch/mount must be cached and rebuilt on change (file-watcher / explicit action), never recomputed per call; heavy/subprocess work goes to `spawn_blocking` so it can't starve the Tauri async runtime and freeze the whole UI.
