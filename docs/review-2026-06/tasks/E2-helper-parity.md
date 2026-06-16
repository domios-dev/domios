# E2 — Parser-helper duplication: accept + guard (arbiter decision)

**Executor:** codex tier · **Branch:** stabilization · **Effort:** S
**Backlog ref:** E2. Decision: post-E1 the duplicate helpers live in the two
hand-maintained QuickJS bundles (plugin.js ×2) which cannot import shared modules.
A bundler build-step is NOT worth the infra for two files (revisit if a 4th
provider appears). Instead: document + parity-test the helpers so drift is caught.

## Step 0 — confirm receipt
```
codeterm send "TASK E2 ACCEPTED, starting" --pane 5c64a651
```

## Steps
1. Identify the helper functions duplicated between
   plugins/provider-claude/plugin.js and plugins/provider-codex/plugin.js
   (candidates: extractText-like, isRecord-like, tool-name normalization,
   id-derivation; confirm against the real files).
2. Add ONE cross-provider parity test (in whichever plugins test file fits):
   for each duplicated helper, feed the same inputs through both providers'
   implementations (load both plugin.js in the test harness the way existing
   tests do) and assert identical outputs over a small input matrix incl. edge
   cases (empty, nested arrays, non-string content).
3. Add a comment block at the top of EACH plugin.js helper cluster:
   "Duplicated by design across provider bundles (QuickJS, no imports) — parity
   guarded by <test name>. Change both or the test fails."
4. Verify: bash scripts/plugin-tests.sh green (incl. new parity test); sabotage
   once (mutate one helper, test must fail, restore).

## Forbidden
- No build-step/bundler introduction. No behavior changes to the helpers.
- Git arbiter-only: report paths + proposed msg
  `test(providers): cross-bundle helper parity guard (E2)`.

## Report
```
codeterm send "TASK E2 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
