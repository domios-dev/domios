# A8 — Fix cli-source-of-truth workflow (has failed on every run, ever)

**Executor:** Haiku or Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** A8 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK A8 ACCEPTED, starting" --pane 5c64a651
```

## Goal
The two cargo jobs in `.github/workflows/cli-source-of-truth.yml` actually execute
their tests instead of dying on "could not find Cargo.toml".

## Evidence (run FIRST; if output differs, report BLOCKED "evidence stale")
```bash
ls Cargo.toml 2>&1              # expected: No such file (repo root has no Cargo.toml)
ls cli/Cargo.toml               # expected: exists
grep -n "working-directory" .github/workflows/cli-source-of-truth.yml   # expected: zero hits
gh run list --workflow=cli-source-of-truth.yml --limit 3                # expected: all FAIL
```

## Steps
1. Read `.github/workflows/cli-source-of-truth.yml` in full. Identify every `run:`
   step that invokes `cargo` (currently: `cargo test -p codeterm-cli cmd::manifest`
   and `cargo test -p codeterm-cli plugin_subcommands_parse`).
2. Add `working-directory: cli` to each cargo step (step-level key, same indent
   level as `run:`). Do NOT touch the `chat-card-tests` job — it passes today.
3. If the cargo jobs lack a Rust toolchain setup step that other workflows have
   (compare with `.github/workflows/file-sizes.yml` and the release workflows),
   note it in the report but change nothing beyond working-directory — one fix at
   a time.
4. Commit: `build: fix cli-source-of-truth cargo jobs (run in cli/, was failing since inception)`
5. Push the branch, then trigger a run to prove the fix:
   `gh workflow run cli-source-of-truth.yml --ref stabilization` — if the workflow
   has no workflow_dispatch trigger, note that the proof will come from the next
   PR's checks instead; do not add new triggers yourself.

## Verify
```bash
git diff HEAD~1 --stat            # must: only .github/workflows/cli-source-of-truth.yml
# If dispatch worked:
gh run list --workflow=cli-source-of-truth.yml --limit 1   # must: pass, or pending then pass
# In the run log: cargo test output with a NON-ZERO test count for cmd::manifest.
```

## Forbidden
- Do not modify test code under `cli/` — if the tests fail once they finally run,
  that's a REAL finding: report it, don't fix or silence it.
- Do not touch other workflow files.

## Report
```
codeterm send "TASK A8 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <did the tests actually run; counts; legit failures if any>" --pane 5c64a651
```
