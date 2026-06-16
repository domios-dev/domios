# CLI-AUDIT — read-only inventory: duplication, dead surface, doc/test gaps

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization (READ-ONLY — no file edits at all)
**Backlog ref:** CLI-AUDIT in ../stabilization-backlog.md (post-merge section)

## Goal
A trustworthy map of the `codeterm` CLI surface: every duplicate/overlapping command, dead legacy path, flag inconsistency, and undocumented/untested command — with a concrete kill-list proposal. No code changes; this report drives post-merge deletion tasks.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK CLI-AUDIT ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1            # expected: c08c2e55 or a descendant on stabilization
git ls-files docs/cli-manifest.json   # expected: prints the path
```

## Steps (in order, no skipping, no additions)
1. Inventory: walk `cli/src/args/` + `cli/src/cmd/` + `cli/src/mesh_cli/api_map.rs` + `docs/cli-manifest.json`. Build one table: command path, flags, REST/socket endpoint it maps to, documented in docs/API.md or docs/CLI.md (yes/no, line ref), covered by any test (yes/no, file ref).
2. Duplication hunt: commands or flags that do the same thing under different names (aliases like `--pane`/`--id`, overlapping verbs, two commands hitting the same endpoint with cosmetic differences); CLI logic that re-implements something the daemon already exposes; api_map entries duplicating cmd implementations.
3. Dead surface: commands referencing removed features (lead role remnants, legacy endpoints), commands whose endpoint no longer exists, flags that are parsed but never used (grep the handler bodies).
4. Consistency: flag naming (`--pane` vs `--id` vs positional), `--json` availability, `--mesh` availability — list commands that deviate from the dominant convention.
5. Kill-list proposal: for each candidate — what to delete, what (if anything) replaces it, risk. No external users: bias toward deletion. Mark anything load-bearing for prod supervisor / GA / cron / mesh relay as KEEP with reason.

## Verify
The report itself is the artifact. Every claim must carry a file:line reference — no "seems unused" without a grep proof (`git grep -n <symbol>` showing zero call sites).

## Forbidden (beyond the global rules in README.md)
- NO file edits, NO git mutations, NO daemon restarts, nothing against prod 7685.
- No fixes "while you're there" — report only.

## Report
Write the full table + kill-list to /tmp/cli-audit-report.md, then:
```
codeterm send "TASK CLI-AUDIT STATUS DONE: report at /tmp/cli-audit-report.md; top findings: <3-5 lines>" --pane 5c64a651
```
