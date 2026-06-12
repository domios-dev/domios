# A7 — Supply-chain hygiene: dependabot + weekly audit job

**Executor:** Haiku pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** A7 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK A7 ACCEPTED, starting" --pane 5c64a651
```

## Goal
Dependabot watches cargo/npm/actions ecosystems (grouped updates); a weekly
report-only audit job runs `cargo audit` + `pnpm audit`.

## Evidence (run FIRST)
```bash
ls .github/dependabot.yml 2>&1        # must: No such file
grep -rn "cargo audit\|pnpm audit" .github/workflows/ build.sh   # must: zero hits
```

## Steps
1. Create `.github/dependabot.yml`: version 2; three update blocks —
   `cargo` (directory `/src-tauri`; also check whether `/cli` and `/crates/*` have
   their own Cargo.toml manifests — list every cargo root you find and cover each),
   `npm` (directory `/`, pnpm workspace), `github-actions` (directory `/`).
   All: weekly, with a `groups:` block bundling minor+patch into one PR per
   ecosystem. Major updates stay individual.
2. Create `.github/workflows/dep-audit.yml`: `on: schedule` (weekly cron) +
   `workflow_dispatch`. Two jobs: (a) `cargo install cargo-audit && cargo audit`
   run from each cargo root found in step 1; (b) pnpm setup + `pnpm audit --prod`.
   `continue-on-error: true` on both — REPORT-ONLY by design, must not gate
   anything yet.
3. Append a short "Dependency advisories" paragraph to
   `docs/contributing/style.md` (assess reachability → bump or pin → note in PR);
   bump its revision tag.
4. NO git commands. Report changed paths + proposed commit message:
   `build: dependabot config + weekly report-only dependency audit`

## Verify
```bash
ls .github/dependabot.yml .github/workflows/dep-audit.yml      # both exist
# yaml sanity:
python -c "import yaml,sys; yaml.safe_load(open('.github/dependabot.yml')); yaml.safe_load(open('.github/workflows/dep-audit.yml')); print('yaml ok')"
```

## Forbidden
- No gating: nothing in this task may fail CI for other PRs.
- Do not bump any dependency yourself.

## Report
```
codeterm send-text "TASK A7 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <cargo roots found>" --pane 5c64a651
```
