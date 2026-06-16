# F1 — web_routes token validation: constant-time compare + single TTL source

**Executor:** Sonnet pane · **Branch:** stabilization (after W0 lands) · **Effort:** S
**Backlog ref:** F1 in ../stabilization-backlog.md

## Goal
The PWA-manifest token path uses the same constant-time comparison and the same TTL
constant as the main auth path; no hardcoded `3600` remains in token logic.

## Evidence (run FIRST; if any check finds nothing, report BLOCKED "evidence stale")
```bash
grep -n "api_token == candidate" src-tauri/src/api/web_routes.rs   # expected: 1 hit (~line 161)
grep -n "from_secs(3600)" src-tauri/src/api/web_routes.rs          # expected: 1 hit (~line 168)
grep -n "Max-Age=3600" src-tauri/src/api/web_routes.rs             # expected: 1 hit (~line 193)
grep -n "WEB_TOKEN_TTL" src-tauri/src/api/routes.rs                # expected: const definition
grep -n "constant_time_eq" src-tauri/src/services/tokens.rs        # expected: pub(crate) fn
```
Line numbers may have shifted (W0 touched these files) — trust the grep, not the
numbers. If a pattern is already fixed, report which ones and do only the rest.

## Steps
1. In `web_routes.rs`, replace the plain `api_token == candidate` comparison with
   `crate::services::tokens::constant_time_eq(api_token.as_bytes(), candidate.as_bytes())`
   (match the exact call shape used in `src-tauri/src/api/auth.rs` — read it first).
2. Replace `std::time::Duration::from_secs(3600)` with
   `crate::api::routes::WEB_TOKEN_TTL`.
3. Replace the literal `3600` inside the `Max-Age=` cookie string with
   `crate::api::routes::WEB_TOKEN_TTL.as_secs()` via the existing `format!` args.
4. Add one unit test in the `web_routes` test module: assert
   `WEB_TOKEN_TTL.as_secs()` appears in the Set-Cookie string builder output AND
   that `validate_manifest_token` rejects a token differing in the last byte
   (exercises the constant-time path's correctness, not its timing).
5. Commit on the current branch: `security: constant-time manifest token compare + derive TTLs from WEB_TOKEN_TTL`

## Verify
```bash
grep -n "3600" src-tauri/src/api/web_routes.rs        # must: zero hits in token/cookie logic
./build.sh check                                      # must: exit 0
# PowerShell foreground, timeout 90s:
cargo test api::web_routes                            # must: pass, includes the new test
```

## Forbidden
- Do not refactor `constant_time_eq` location (that's task F4's territory).
- Do not touch `auth.rs` beyond reading it.

## Report
```
codeterm send "TASK F1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
