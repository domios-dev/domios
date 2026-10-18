# Chat send, delivery and Stop

Purpose: prove one input reaches the intended own session once and stays one visible conversation item. Core: one owned Codex runtime plus Claude PTY, short send and Stop/reload.

## Preconditions and fixtures

Use the shared DEV setup. Discover `dc agent providers` and `dc agent models <provider>`; owner grants fixture spawns and model budget. Provision a unique workspace using `dc workspace create Qachat --provision --json`. Save the returned ID/cwd. Spawn through `dc agent spawn <provider> --workspace "$WS" --title Chatprobe --description "Owned delivery QA" --task-file "$PAD/fixture-task.md" --json`. The pad body asks for an opaque reply and no other tools. Record tab/session/backing/nonce before sends using `dc tab inspect "$TAB" --json` and `dc tab chat --tab "$TAB" --json`.

## Steps / evidence

1. In the own Domios Browser, open DEV web mirror, authenticate via process memory, select only the owned workspace/tab, snapshot. Use `dc --as-tab "$OWNER" plugin browser ...`; never use a host tab as DEV caller. Scope DOM reads to the visible `[data-tab-id="OWN_ID"]` pane.
2. Type/paste one unique pad-authored marker into that pane's composer and submit once. Require composer clears immediately, a sending/delivery indicator next to one bubble and the next draft remains editable. Use DOM roles/names and real input; clipboard overwrite needs separate consent.
3. Capture the exact own-session transcript with `dc tab chat --tab "$TAB" --json`; require one matching user item, one expected final assistant item, new IDs, same session and idle. A screen echo or a user item is not an assistant result. For CLI delivery, use `dc --as-tab "$OWNER" send --tab "$TAB" --from-pad marker.md --json`, retaining receipt/send ID and raw exit. If written/held/submit_unconfirmed, classify and inspect exact session evidence; never resend it to make it pass.
4. Send two distinct markers once to two owned recipients. Read each far-end session; require one intended marker each, no cross-talk, and corresponding Sent-to cards with recipient, exact body and final delivery state. A Success card alone is insufficient.
5. Start an explicitly bounded long turn. On a real working event, `dc tab stop --tab "$TAB" --json`; require stopped=true, same-session idle, persisted interrupted presentation and no replay. Reload the view and recount the original input once. A live background terminal may outlast a finished parent; verify parent status against turn-completed evidence, then explicitly stop the owned child if granted.
6. For UI polish subcases, inspect real hover at desktop1440 and touch-sized768/390: timestamp shown on desktop hover, hidden at narrow widths; compact tool-card groups and no duplicate strip. Record actual viewport and animations finished; screenshot alone cannot prove send counts.

PASS requires the API/session and DOM agree. FAIL: duplicate/missing turn, wrong session, false delivery, stuck composer or replay. NOT RUN: absent provider, no working-event fixture, unsupported physical input, or missing grant. Keep uncertain delivery separate from proven non-delivery.

Cleanup: close owned Browser, child processes and tabs; close the owned workspace after saving evidence. Never remove provider session history or foreign fixtures.

Seeds: Turing reports padded/pasted single-turn delivery, two CLI sends/Sent-to, timestamp widths and Claude interruption/restart PASS. Keel's early input/restore records are historical and partially superseded. Internal subagent Stop and restored transcript409 were open Turing failures, not covered by an ordinary parent Stop PASS.

Automation: reuse session-count and delivery-ledger mechanisms in existing `evals/delivery-confirmation` / runtime-provider matrix. UI inspection proves wiring; unit tests own decisions. Keep explicit provider/own-thread binding, not latest-global session lookup.
