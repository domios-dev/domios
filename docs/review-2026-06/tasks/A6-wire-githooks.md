# A6 — Wire .githooks into git config (hooks currently run for nobody)

**Executor:** Haiku or Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** A6 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK A6 ACCEPTED, starting" --pane 5c64a651
```

## Goal
`git config core.hooksPath` points at `.githooks` after any `./build.sh` invocation;
the pre-push hook's content is sane (<30s, Windows-safe).

## Evidence (run FIRST)
```bash
git config core.hooksPath        # expected: .git/hooks or empty — NOT .githooks
ls .githooks/                    # expected: pre-push exists
cat .githooks/pre-push           # read it; note what it runs and rough duration
```

## Steps
1. In `build.sh`, find the common entry point that every subcommand passes through
   (near the top, after arg parsing). Add an idempotent one-liner:
   `git config core.hooksPath .githooks 2>/dev/null || true`
   Mirror the same in `build.ps1` if it has an equivalent shared path.
2. Time the hook: if what it runs (likely `cargo check`) exceeds ~30s cold, note it
   in the report — do NOT change the hook's content (that's A4's coordination).
3. Document in `docs/BUILD.md` (one short paragraph: hooks live in `.githooks/`,
   wired automatically by build.sh; bump the revision tag).
4. Commit: `build: auto-wire core.hooksPath to .githooks from build.sh`
5. Demonstrate once: make a scratch commit with a deliberate compile error on a
   throwaway branch, attempt `git push --dry-run`-style push to confirm the hook
   fires and rejects; then reset/delete the scratch branch fully. Paste the
   rejection output. (If the hook only triggers on real push, push a scratch branch
   `tmp-a6-hooktest` to origin, then delete it remotely: `git push origin
   --delete tmp-a6-hooktest`.)

## Verify
```bash
./build.sh check >/dev/null; git config core.hooksPath    # must print: .githooks
grep -n "hooksPath" build.sh docs/BUILD.md                # both present
```

## Forbidden
- Do not edit the hook's checks themselves.
- Clean up the scratch branch both locally AND on origin.

## Report
```
codeterm send-text "TASK A6 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <hook duration, content notes>" --pane 5c64a651
```
