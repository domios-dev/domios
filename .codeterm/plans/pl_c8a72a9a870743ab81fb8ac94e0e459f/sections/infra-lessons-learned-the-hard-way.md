**HOW we learned to keep the daemon responsive.** The headline lesson: blocking/heavy work on the Tauri async runtime starves *all* IPC and freezes the whole UI — a 3–4s full-UI freeze on every tab switch turned out to be `plugin_manager_snapshot` forking a CLI subprocess (a network `channel diff`) on the runtime. The canonical fix is a stale-but-instant cache that never computes on the caller, shown in `commands/plugins/diff_cache.rs`:

```rust
const DIFF_REFRESH_AFTER: Duration = Duration::from_secs(300);
// Computing the diff does a per-channel git clone (4-7s), so it must NEVER run inside
// the snapshot hot path — stale-but-instant beats fresh-but-blocking.
fn read<P, S>(&self, produce: P, spawn: S) -> Value where P: Fn() -> Result<Value,String> + Send + 'static, S: FnOnce(P) {
    let snapshot = self.cache.lock().ok().and_then(|g| g.clone());
    let stale = self.force.load(SeqCst)
        || snapshot.as_ref().map(|(at,_)| at.elapsed() >= DIFF_REFRESH_AFTER).unwrap_or(true);
    if stale && self.refreshing.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
        self.force.store(false, SeqCst);
        spawn(produce);                 // background thread in prod, inline in tests
    }
    snapshot.map(|(_, v)| v).unwrap_or(STUB)   // cold first call returns a stub, never blocks
}
```

The hard-won rules, applied across the plan store: (1) the daemon NEVER shells out to its own `codeterm` CLI — call the in-process function; a daemon forking a CLI that reconnects to it is an architecture smell and slow. (2) Blocking/IO/heavy/subprocess work in a Tauri command goes to `spawn_blocking`. (3) Hot-path commands (fire on tab switch/mount/render) must be cheap and bounded — cache expensive snapshots, rebuild on change (the file-watcher), never per call. (4) Measure: the dev build logs `[SLOW-IPC]` / `[LONGTASK]`, and the command that resolves *first* is the cause — the rest are queued victims.

## Key decisions
- **Stale-but-instant over fresh-but-blocking** — a hot path returns last-known immediately and refreshes in the background; a cold call returns a stub rather than ever blocking.
- **Single-flight refresh** (`refreshing` CAS) — only one background recompute at a time; a mid-refresh action sets `force` so the next read refetches.
- **In-process over CLI fork** — the plan store is called directly; no process start + IPC + (worse) network per call on the runtime.
