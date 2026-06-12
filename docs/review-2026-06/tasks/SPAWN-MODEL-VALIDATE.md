# SPAWN-MODEL-VALIDATE — bogus model ids must fail loudly at spawn

**Executor:** fresh codex worker · **Branch:** stabilization · **Effort:** S/M
**Backlog ref:** ../model-fidelity-report.md (negatives section)

## Goal
`codeterm agent spawn <provider> --model <bogus>` fails with a clear error and creates NO pane, instead of today's EXIT=0 + broken/wrong-model TUI (opencode silently boots its default MiMo — the worst case: user asked for X, got Y, nobody told them).

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK SPAWN-MODEL-VALIDATE ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1 -- docs/review-2026-06/model-fidelity-report.md   # expected: report at HEAD
codeterm agent models opencode | head -5                               # expected: model list with opencode/ prefixed ids
```

## Design decisions (final)
- Validation lives in the DAEMON spawn path (orchestrator/spawn params), not the CLI — every surface (CLI/REST/socket/mesh) gets it for free.
- Validate ONLY when the provider can enumerate models (the same source `agent models <provider>` uses). Unknown id + enumerable list → reject: clear error naming the bad id and pointing to `codeterm agent models <provider>`; NO pane created.
- Providers that accept aliases (claude: haiku/sonnet/opus + full ids) must keep accepting them — validate against the full alias+id set the provider advertises.
- If a provider cannot enumerate (dynamic/unknown) → pass-through unchanged (document this in the error-path comment via code structure, not prose).
- Bonus trap from the probe: bare `big-pickle` vs `opencode/big-pickle` — if the provider's list contains a prefixed id whose suffix matches the given bare id uniquely, reject with a "did you mean opencode/big-pickle" style error (no silent auto-correct).

## Steps
1. Read how `agent models` enumerates per provider; add a daemon-side validation hook in the spawn path before pane creation.
2. Unit tests: valid id passes; bogus id rejected with no pane; alias accepted (claude); unique-suffix near-miss produces the suggestion error; non-enumerable provider passes through.
3. CLI already surfaces REST errors — verify the error text reaches the user (manual run against dev is DEFERRED to the dev window; prod runs the old daemon, do not probe prod for this).
4. docs/API.md: one line on spawn model validation behavior; revision bump.

## Verify (paste real output)
```
cargo test --manifest-path src-tauri/Cargo.toml <narrow filters>      # must: pass
& "C:\Program Files\Git\bin\bash.exe" build.sh check                  # must: exit 0
& "C:\Program Files\Git\bin\bash.exe" -c 'bash scripts/check-file-sizes.sh --fail'  # must: exit 0
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations. NO touching evals/, tests/playwright/, detection code (DETECT-MODEL-FIX owns src-tauri/src/plugin/*/provider.rs detection fns — coordinate: your changes live in the spawn/orchestrator path; if you must touch the same provider files, ONLY add enumeration helpers, never edit detect_session_model).
- No dev:7686, no prod spawns.

## Report
```
codeterm send-text "TASK SPAWN-MODEL-VALIDATE STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
