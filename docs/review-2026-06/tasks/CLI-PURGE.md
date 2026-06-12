# CLI-PURGE — execute the audit kill-list + fix mesh relay lies

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization · **Effort:** M/L
**Backlog ref:** CLI-AUDIT in ../stabilization-backlog.md · **Source:** ../cli-audit-report.md (your own audit, arbiter-verified)

## Goal
The CLI surface contains zero known duplicates, zero stale lead/conductor remnants, and the mesh relay never silently lies (no wrong-endpoint mappings, no fake fallbacks, no silently dropped flags).

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK CLI-PURGE ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1 -- docs/review-2026-06/cli-audit-report.md   # expected: f9d1ca65
git grep -n "set-conductor" -- cli/src/args/agent.rs              # expected: alias still present (line ~23)
```

## Arbiter decisions on contested items (final, do not re-litigate)
- **KEEP** `get-output` and the `send-text` alias — primary muscle-memory surface for every agent prompt, skill doc, and GA. Folding comes later if ever.
- **KEEP** `debug crash` (crash harness) and `bridge restart` as-is.
- Everything in the DELETE list below is approved for deletion — no deprecation shims, no aliases-to-warnings. Delete means delete.

## Steps (in order, no skipping, no additions)
1. DELETE: `agent list`, `agent hierarchy`, `set-conductor` alias, `paste pop`, `mesh send` subcommand, top-level `update --check/--apply/--channel` shortcut flags. Replacements already exist (`pane list --agents`, `paste get --pop`, `send --mesh`, `update check/install/channel`).
2. PURGE stale wording: every lead/Lead mention in cli help strings (`cli/src/args/agent.rs:22,:35` and any others `git grep -in "lead" -- cli/src` finds in user-facing strings).
3. FIX mesh relay correctness:
   - `capabilities` over mesh: stop mapping to `/api/identify`. Either expose a real REST capabilities endpoint mirroring `system.capabilities` (preferred if small) or return UNSUPPORTED like the plugin family.
   - mem graph/dream local-only commands: kill the `/api/mem/stats` fallback (`api_map.rs:927,:1042`) → UNSUPPORTED pattern from `:1050`.
   - `pane new --cmd`: relay it (REST create accepts cmd? verify; if endpoint lacks it, add the field) — remote must match local semantics.
   - `mem update --changed-by`: relay it.
   - `update install --from-file/--continue-prompt/--continue-pane`: explicit local-only error over mesh, not silent drop.
4. Sweep references to deleted commands: `git grep -n "agent list\|agent hierarchy\|paste pop\|mesh send\|set-conductor"` across docs/, evals/, scripts/, tests/, .codeterm/, src-tauri/prompts/ — update each (note: docs/SKILLS.md and the codeterm skill reference `agent hierarchy`; replace with `pane list --agents`).
5. Regenerate: `codeterm cli-manifest --write` equivalent via the local build (cargo run -p codeterm-cli -- cli-manifest --write or the documented path), regen docs/CLI.md if generated, bump revisions on touched docs.
6. Tests: update/extend `cli/src/mesh_cli/tests.rs` for every mapping you changed (capabilities, mem graph UNSUPPORTED, pane new cmd, mem update changed-by, update install local-only). Deleted commands: manifest/shape tests must not reference them. Add one test asserting `git grep -c "lead"` style absence is NOT needed — instead assert help text of `agent set-orchestrator` contains "workers" and not "lead".

## Verify (run all; paste real output in the report)
```
cargo test --manifest-path cli/Cargo.toml                          # must: all pass (PowerShell foreground, 90s cap; narrow first if slow)
& "C:\Program Files\Git\bin\bash.exe" build.sh check               # must: exit 0
& "C:\Program Files\Git\bin\bash.exe" build.sh cli                 # must: builds; then codeterm --help shows no deleted commands
git grep -n "set-conductor\|paste pop" -- cli/ docs/                # must: empty
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations — report changed paths + proposed commit message; I commit.
- Do not touch evals/session-restore or evals/restore-matrix (another lane owns them).
- No scope creep into the `--pane`/`--id` flag-consistency standardization — that's a separate future task; this one is deletions + relay truth only.

## Report
```
codeterm send-text "TASK CLI-PURGE STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
