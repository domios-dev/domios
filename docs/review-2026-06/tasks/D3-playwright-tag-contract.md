# D3 — Playwright tag contract: projects in config + untagged-test lint

**Executor:** Gpt5.5 pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** D3 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK D3 ACCEPTED, starting" --pane 5c64a651
```

## Goal
Tag tiers are runnable (`pnpm test:pw:smoke` / `:golden` / `:full`) and an untagged
test cannot land unnoticed (lint fails on it). `workers: 1` stays — projects
partition by tag, not parallelism.

## Evidence (run FIRST)
```bash
cat tests/playwright/playwright.config.ts        # no projects block; retries: 0; workers: 1
grep -rn "@smoke\|@golden" tests/playwright/suites/ | head -5   # tags exist in titles
grep -n "test:pw" package.json                   # only the bare test:pw script
```

## Steps
1. Add to `playwright.config.ts` a `projects` array (keep every existing top-level
   option untouched, including `workers: 1`):
   - `{ name: "smoke",  grep: /@smoke/ }`
   - `{ name: "golden", grep: /@golden/, retries: 1, timeout: 300_000 }`
   - `{ name: "full",   grepInvert: /@quarantine/ }`
2. Add to root `package.json` scripts:
   `"test:pw:smoke": "playwright test -c tests/playwright --project=smoke"`,
   same for `golden` and `full`.
3. Create `tests/playwright/helpers/check-tags.mjs` — a ~30-line node script:
   scan `suites/**/*.spec.ts` for `test(` titles; exit 1 listing any title that
   contains none of `@smoke|@golden|@quarantine`; exit 0 otherwise. No deps beyond
   node stdlib.
4. Add script `"test:pw:lint": "node tests/playwright/helpers/check-tags.mjs"` and
   run it. If existing tests are untagged — do NOT tag them yourself; list them in
   the report (the arbiter assigns tiers).
5. Create `tests/playwright/QUARANTINE.md` with the header: columns test / reason /
   owner / date / exit criteria, and the policy line "quarantined >30 days with no
   progress → delete" (no entries yet).
6. Update the tags section of `tests/playwright/README.md` to point at the new
   scripts (do not reintroduce a taxonomy — D6 moved that to docs/TESTING.md).
7. NO git commands. Report changed paths + proposed commit message:
   `test(pw): tag-tier projects, smoke/golden scripts, untagged-test lint, quarantine policy`

## Verify
```bash
pnpm test:pw:lint                       # exit 0 OR a list of untagged tests (paste either)
npx playwright test -c tests/playwright --project=smoke --list | head -10   # only @smoke tests listed
node -e "1"                             # sanity that node runs scripts on this machine
```

## Forbidden
- Do not run the actual UI suites (dev may be down; `--list` is enough).
- Do not modify any spec file.
- Do not change `workers`, `globalSetup`, or reporters.

## Report
```
codeterm send "TASK D3 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <untagged test list>" --pane 5c64a651
```
