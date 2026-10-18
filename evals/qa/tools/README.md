# Draft helper inventory

Run from the repository root after the owner reviews/copies this directory to `evals/qa/`. All helpers are drafts, not installed gates. Generalized parameter versions below have only syntax checks; Joy's original fixed-fixture run supplies the behavioral seed, not live proof of these versions.

- `dev-session.sh`: source after the repository bench library; `qa_run LABEL dc ...` records the exact nonsecret command, stdout, stderr and raw exit separately, refusing evidence overwrite. Set QA_OUT to a fresh run directory. Do not use this logger for secrets, account login, web links or challenge-bearing Browser pages.
- `dev-request.mjs`: DEV-only JSON HTTP capture; GET on a narrow evidence surface and POST only to create a disposable cron. Body comes from a file, auth/caller from dev_run environment. It does not authorize a fixture or a new route.
- `browser-fixture-server.mjs`: external real-form tests come first. This controlled loopback fixture then exposes field states, SPA, iframe, popup and an event-gated navigation. Start in an owned `tab new --purpose dev --cmd ...` server utility; if that fixture has no Team, record its explicit owner and containment. Never start a compiler. Args: `--out ABSOLUTE_DIR [--port PORT]`; default port0 selects an unused port. Read its ready.json, confirm the actual HTTP page and pin process/tab IDs. No timers control readiness.
- `held-navigation-check.mjs`: DEV-scoped probe using dc inside a sourced Bash child. Args: `--bash ABSOLUTE_BASH --owner TAB --origin http://127.0.0.1:PORT --out ABSOLUTE_FILE`. Require a fresh snapshot of the fixture's Held navigation link before running. It waits on /gate, observes early click completion before releasing the real response, releases in finally, and then explicitly waits/snapshots. An early return records a mismatch; it does not decide whether the product promises implicit settling. No callback/live payload is replayed.
- `cdp-owned-page.mjs`: supplementary DOM capture on the same agent-owned Browser. First `dc --as-tab "$OWNER" plugin browser endpoint > "$OUT/endpoint.txt"`. Then `dev_run --as-tab "$OWNER" node evals/qa/tools/cdp-owned-page.mjs --endpoint-file "$OUT/endpoint.txt" --url "$OWN_PAGE_URL" --tab "$OWN_TAB" --phase install --expression evals/qa/tools/reset-observer.js --out "$OUT/observe-install.json"`. It refuses prod/external pages and ambiguous targets; source the bench lib first. Endpoint files are local control capabilities; keep them in the run directory and remove after capture. Never use global browser targets or a foreign Browser endpoint.
- `reset-observer.js`: function expression consumed by the CDP helper. Phase install wraps fetch only on the owned runtime reset path and records action/confirmed flags; phase read returns request counts and the visible usage dialog; phase restore returns evidence and restores fetch. Run restore in cleanup even after a failed UI command. It captures a web-mirror REST path, not native Tauri IPC. For native UI use a separate authorized transport observer.
- `history.md`: executed original evidence and the failed native selection probe, intentionally not a reusable driver.

Safe examples, after filling actual owned IDs from responses:

```bash
unset CODETERM_CLI_PATH
source evals/_lib/lib.sh
export QA_OUT="$PWD/target-check/evals/qa/unique-run"
source evals/qa/tools/dev-session.sh
qa_run health dev_get /api/health
qa_run form dc --as-tab "$OWNER" plugin browser snapshot
# Body file contains only the explicitly owned synthetic cron fields.
dev_run --as-tab "$OWNER" node evals/qa/tools/dev-request.mjs POST /api/crons "$PAD/cron.json" "$QA_OUT/cron-created.json"
```

All live changes require the caller's existing grant. No helper starts/stops an app, creates a named profile, sends a chat/bridge message, reads account secrets or consumes a reset. A bounded failure deadline can stop observation; it never makes a page ready. Parent reviews draft helper correctness and integration/catalogue registration before these are executed as a new gate.
