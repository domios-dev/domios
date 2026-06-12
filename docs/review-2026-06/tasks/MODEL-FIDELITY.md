# MODEL-FIDELITY — does `agent spawn --model X` provably boot X?

**Executor:** fresh codex worker · **Branch:** stabilization · **Effort:** M
**Backlog ref:** MODELS-PROBE in ../stabilization-backlog.md (this is the verification slice; the nightly cron comes later)
**User report:** spawning a specific agent with a specific model via CLI previously booted the wrong model, and the CLI/skill/prompt description was unclear enough that an agent could not figure out the correct invocation. Live suspicion right now: a matrix pane requested big-pickle but its tab chip shows MIMO-V2.5-FREE.

## Goal
For every provider, `codeterm agent spawn <provider> --model <m>` boots exactly model `m` (proven structurally, not by asking the model), a bogus model id fails loudly instead of silently booting a default, and the discovery surface (`--help`, the codeterm skill, src-tauri/prompts/codeterm_cli.md) teaches the correct invocation.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK MODEL-FIDELITY ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
codeterm agent providers          # expected: claude, codex, opencode (+ any plugins)
codeterm agent models codex       # expected: a model list incl. gpt-5.5
codeterm agent models opencode    # expected: a model list (note whether big-pickle is listed)
```

## Environment rule
dev:7686 is OWNED by the restore-matrix lane — do not touch it. All live probes run against PROD (the instance you are in), inside ONE disposable workspace `MODEL-FIDELITY-<runid>`. Close it at the end on success; on failure leave it and report pane ids.

## Steps (in order, no skipping, no additions)
1. READ the plumbing first: how `--model` travels (cli args → spawn REST/socket → daemon → provider launch command) and how the booted model is DETECTED (pane model chip / `codeterm agent status` / structural TUI detection). Name the files+lines in the report. The proof method must be structural (what the daemon detected), not the model's self-report.
2. PROBE matrix in the disposable workspace: codex × {gpt-5.5, gpt-5.4-mini}, opencode × {big-pickle, one other free model}, claude × {haiku ONLY — budget; spawn, verify detection, no prompt needed if detection works without one}. For each pane record: requested model → launch command actually built → detected/booted model → MATCH or MISMATCH.
3. NEGATIVE: spawn each provider with a bogus model id (e.g. `--model no-such-model-xyz`). Must fail loudly (non-zero exit / clear error). If it silently boots a default — that is a FINDING, report it, do not fix.
4. The user's live case: check pane rm-opencode-1 in workspace RESTORE-MATRIX-32010939ab (prod-visible? it's on dev — if unreachable from prod, skip with reason): was it spawned with --model big-pickle and booted MIMO? If reachable, pull its spawn params from the snapshot/logs.
5. DISCOVERABILITY audit: `codeterm agent spawn --help` output, the codeterm skill section on spawn, and src-tauri/prompts/codeterm_cli.md — would an agent reading only these construct the right command (incl. discovering model ids via `agent models <provider>`)? Fix unclear wording in repo files (live docs/prompts only); report exact gaps.
6. CLEANUP: close the disposable workspace (success path only).

## Verify (paste real output in the report)
- The per-pane MATCH/MISMATCH table with the structural evidence for each row.
- Bogus-model spawn outputs for all three providers.

## Forbidden (beyond the global rules in README.md)
- NO git mutations. NO daemon/CLI code changes — findings over fixes this round (docs/prompt wording fixes ARE allowed).
- Claude: haiku only, zero or one prompt max.
- Never touch dev:7686, never kill anything, never spawn outside the disposable workspace.

## Report
```
codeterm send-text "TASK MODEL-FIDELITY STATUS ... TABLE: <requested -> built cmd -> detected, MATCH/MISMATCH per row> NEGATIVES: ... DISCOVERABILITY: ... CHANGED: ... NOTES: ..." --pane 5c64a651
```
