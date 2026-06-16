# A1 — Split src-tauri/src/mem/dreaming.rs into a module (drain its allowlist entry)

**Executor:** Opus pane · **Branch:** stabilization · **Effort:** L
**Backlog ref:** A1 in ../stabilization-backlog.md

## Goal
`dreaming.rs` (1492 lines, re-baselined in W0 as a stopgap) becomes a `dreaming/`
module where every file is <1000 lines (`mod.rs` <600), and its allowlist entry is
REMOVED. Zero behavior change.

## Evidence (run FIRST)
```bash
wc -l src-tauri/src/mem/dreaming.rs                    # expected: ~1492
grep -n "dreaming.rs" scripts/file-size-allowlist.txt  # expected: 1 entry, baseline 1492
```

## Steps
1. Read the whole file. Map responsibilities into 3–5 groups. Expected seams
   (verify against the real code, do not force them): schedule management /
   REM-stage orchestration / prompt assembly / session+store manipulation.
2. Create `src-tauri/src/mem/dreaming/` with `mod.rs` (types, public facade,
   re-exports — keep the external path `crate::mem::dreaming::*` IDENTICAL) and one
   file per group. Move code verbatim — this is `git mv`-grade relocation, not a
   rewrite. Resist every temptation to "improve" logic in passing.
3. If the file has an inline `#[cfg(test)] mod tests`, keep `mod tests;` in
   `mod.rs` and move the body to `dreaming/tests.rs`.
4. Delete the `src-tauri/src/mem/dreaming.rs` entry from
   `scripts/file-size-allowlist.txt` (both the line and its comment block).
5. After the move: `grep -rn "\.lock()" src-tauri/src/mem/dreaming/` and confirm no
   nested lock acquisition appeared (lock order: mux → output_buf → screen_state →
   agent_tracker → pty; parking_lot is non-reentrant).
6. Commit: `mem: split dreaming.rs into dreaming/ module (file-size discipline, A1)`

## Verify
```bash
bash scripts/check-file-sizes.sh --fail        # must: exit 0, no dreaming entries flagged
./build.sh check                               # must: exit 0
# PowerShell foreground, timeout 90s:
cargo test mem::dreaming                       # must: all pass, same test count as before the split (count them BEFORE starting)
grep -rn "use crate::mem::dreaming" src-tauri/src/ cli/src/ | head   # must: call sites unchanged (compare to a pre-split snapshot you took)
```

## Forbidden
- No public-API rename, no signature changes, no logic edits, no new dependencies.
- Do not touch `src-tauri/prompts/dreaming_rem.md` or any CLI/REST surface.
- If the seams don't decompose cleanly below 1000 lines per file → report BLOCKED
  with your seam map instead of forcing an ugly split.

## Report
```
codeterm send "TASK A1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <seam map + test counts before/after>" --pane 5c64a651
```
