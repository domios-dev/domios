# F7 — Fix `workspace create --cwd`: name ignored, existing ws returned, tab leaked

**Executor:** Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** F7 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK F7 ACCEPTED, starting" --pane 5c64a651
```

## Goal
`workspace create NAME --cwd PATH` where PATH matches an existing workspace's cwd:
(a) never leaks a tab, (b) tells the caller explicitly that it returned an existing
workspace. Creating without `--cwd` produces consistent state (no tab_count=1 with
empty pane_ids).

## Evidence (re-confirm in CODE; the live repro on prod is documented in the backlog)
```bash
grep -rn "fn.*create" src-tauri/src/api/routes.rs | grep -i workspace
grep -rn "workspace" src-tauri/src/commands/ --include="*.rs" -l
# Find the create handler; read it; locate the cwd-dedupe branch and the tab creation.
```

## Decision (pre-made by the arbiter — implement as written)
Dedupe-by-cwd STAYS (it's reasonable UX for `codeterm` CLI use from a project dir),
but: (1) when deduping, do NOT create any tab; (2) response gains an
`"existing": true` field; (3) the CLI prints
`workspace with this cwd already exists: <name> (<id>)` and exits 0.
The null-cwd path must initialize counters consistently (tab_count == pane_ids.len()).

## Steps
1. Locate and read the daemon create handler + the CLI-side rendering.
2. Implement the three changes above + the counter fix.
3. Add a pytest-e2e case in `tests/e2e/` (existing-cwd dedupe → existing:true, no
   new tab; fresh create → consistent counters). Follow the conventions of the
   neighbouring tests in that directory.
4. Verify live against DEV only: `./build.sh dev --wait`, run the original repro
   twice (`workspace create "ftest" --cwd <repo>` ×2), confirm zero stray tabs and
   the explicit message; `./build.sh kill`.
5. Commit: `fix(workspace): no tab leak + explicit existing-workspace response on cwd dedupe`

## Verify
```bash
./build.sh check                       # exit 0
# PowerShell foreground 90s:
cargo test workspace                   # incl. anything new
# paste the dev repro output (two create calls, pane list before/after)
```

## Forbidden
- Port 7685. The original bug was found ON PROD — do not "re-verify" it there.
- Do not change workspace focus/close/rename semantics.

## Report
```
codeterm send "TASK F7 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
