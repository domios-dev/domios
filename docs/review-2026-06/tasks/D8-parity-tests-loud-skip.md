# D8 — Parity tests must not skip silently

**Executor:** any claude pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** D8 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK D8 ACCEPTED, starting" --pane 5c64a651
```

## Goal
`js_runtime` parity tests fail loudly in CI when the bundled JS plugins are absent,
and print an explicit SKIPPED line locally — never a silent early-return.

## Evidence (run FIRST)
```bash
grep -n "return" src-tauri/src/plugin/js_runtime/parity_test.rs | head -10
# find the guard that early-returns when the plugins dir is missing (the audit called it ws_exists-style)
grep -rn "CI" src-tauri/src/plugin/js_runtime/parity_test.rs    # expected: no CI-awareness today
```

## Steps
1. Read the guard logic in `parity_test.rs`. Identify every test (or shared helper)
   that early-returns when plugin files are missing.
2. Centralize into one helper: `fn require_plugins() -> Option<PluginsDir>`:
   - if plugins present → Some(...)
   - if absent and `std::env::var("CI").is_ok()` → `panic!("parity tests require bundled JS plugins in CI — build them in the workflow or fix the path")`
   - if absent locally → `eprintln!("SKIPPED: js_runtime parity — plugins dir not found at <path>")` and None.
3. Replace each silent return with the helper.
4. Check whether any CI workflow currently runs `cargo test` on src-tauri (as of
   the audit: none did — tests.yml is task D7, may not exist yet). If none does,
   note in the report that the CI=panic path activates when D7 lands — that's
   expected, not a defect.
5. Commit: `test(parity): loud skip locally, hard fail in CI when JS plugins missing`

## Verify
```bash
./build.sh check                                   # exit 0
# PowerShell foreground, 90s:
cargo test plugin::js_runtime::parity              # all pass (plugins present locally), count unchanged vs before
# Temporarily rename the plugins dir, run one parity test, confirm the SKIPPED line prints, rename back. Paste both outputs.
```

## Forbidden
- Do not weaken or modify any parity assertion.
- Do not touch the plugins themselves.

## Report
```
codeterm send "TASK D8 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
