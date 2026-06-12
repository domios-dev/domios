# E6 — Detection fingerprints: define "structural", harden the literals

**Executor:** codex tier · **Branch:** stabilization · **Effort:** M
**Backlog ref:** E6. Arbiter design decision baked in below — execute, don't re-design.

## Step 0 — confirm receipt
```
codeterm send-text "TASK E6 ACCEPTED, starting" --pane 5c64a651
```

## Design (decided)
"Structural" = detection survives any SINGLE vendor copy change. Concretely: each
provider keeps a SIGNAL LIST where each signal has a weight; detection fires at
threshold; no single literal string may carry >50% of the threshold. Signal kinds,
strongest first: (1) process/title patterns; (2) UI glyphs (✱ ✻ spinner chars);
(3) layout shapes (footer line with N segments, prompt-char column); (4) literal
copy strings (weakest — keep as ONE signal each, never the sole gate).

## Steps
1. Add the definition above (prose, ~10 lines) to docs/contributing/architecture.md
   under the existing detection section; bump revision.
2. Refactor `plugins/provider-claude/src/detect.ts` + the OUTPUT_FINGERPRINTS array
   in `plugins/provider-claude/plugin.js`, and the codex `"codex/"` check in
   `plugins/provider-codex/src/index.ts` (+ its plugin.js twin) to the weighted
   model. Keep ALL current literals as weight-1 signals; add glyph/layout signals
   so threshold is reachable without any one literal.
3. Tests: for each provider, a fixture-driven test that (a) current real output
   detects; (b) the same output with ANY single literal string mutated still
   detects; (c) unrelated terminal output does NOT detect. Reuse existing fixture
   patterns in plugins/*/tests/.
4. Verify: bash scripts/plugin-tests.sh green; pnpm build green.

## Forbidden
- Do not change detection thresholds' observable behavior on current fixtures
  (no false-positive regressions — test (c) guards this).
- Git arbiter-only: report paths + proposed msg
  `fix(detect): weighted multi-signal fingerprints — no single-literal gates (E6)`.

## Report
```
codeterm send-text "TASK E6 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
