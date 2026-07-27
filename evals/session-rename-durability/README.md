# Session rename durability

Proves workspace/tab renames survive an immediate hard kill without waiting
for the 30s client autosave timer.

## Running

Prereqs: dev daemon on port 7686 (`./build.sh dev`).

```bash
bash evals/session-rename-durability/run.sh
```

Flow:

1. Create `WS-rename-durability` workspace + shell tab.
2. Rename workspace and tab via REST API.
3. Assert `last.json` already reflects the new names.
4. `kill -9` / `taskkill /F` the dev daemon immediately.
5. Restart dev and verify names intact in API + `last.json`.

Companion unit tests: `cargo test service::session_persist`.

<!-- revision: 1 -->
