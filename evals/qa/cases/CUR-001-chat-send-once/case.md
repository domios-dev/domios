# CUR-001 — Chat send once and recipient isolation

## Purpose

Prove one authored input reaches its intended provider session once, yields one final answer, and appears as one conversation item. The two-recipient extension checks that concurrent recipients do not cross-talk.

## Covers

Plan task keys: task_tfrk57 covers the comprehensive wave's tested Codex Chat and Claude PTY single-send/reload cell. Its identical-send pair cells remain NOT RUN.

Defects: wrong-session delivery, duplicate or missing turns, false delivery state, and recipient cross-talk. Migrated from chat-send.md steps 1–4.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-browser-auth, dev-fixture-spawn, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the shared DEV setup. The owner grants fixture spawns and model budget. Discover providers and models through the DEV caller. Use an owned DEV Browser and workspace; host tab IDs are not DEV identities. Store credentials only in process memory. The recipe does not authorize clipboard replacement or a production send.

## Fixture

Create a uniquely named workspace and save its returned ID and cwd. Spawn owned Codex runtime and Claude PTY fixtures with a pad-authored task asking for an opaque reply and no other tools. Record each tab, provider session, backing, and binding nonce before sending.

## Steps

1. Unset CODETERM_CLI_PATH, source evals/_lib/lib.sh, and use only dc, dev_get, or dev_run for DEV actions. Set QA_OUT to a new run directory before sourcing the QA logger.
2. In the owned Browser, open the DEV web mirror, authenticate through process memory, select only the owned workspace and tab, and snapshot the visible pane.
3. For the web surface, type one unique pad-authored marker into that pane's composer and submit once. For the CLI surface, send one unique pad-authored marker with dc --as-tab OWNER send --tab TAB --from-pad marker.md --json. Run each surface as its own matrix row.
4. Require the composer to clear immediately for web input. Capture the exact own-session transcript and require one matching user item, one expected final assistant item, new IDs, the same session, and idle state. A screen echo or user item alone is insufficient.
5. As the recipient-isolation extension, send two distinct markers once to two owned recipients. Read each far-end session and require one intended marker each, no cross-talk, and one Sent-to card per recipient with the exact body and final delivery state.
6. If an input is held, written, or submit_unconfirmed, classify it from exact session evidence. Never resend it to make the case pass.

## Oracle

PASS — each selected surface/provider has one exact own-session user marker and one expected final answer, with matching IDs, one visible bubble, and no recipient cross-talk when the extension is run.

FAIL — a duplicate or missing turn, wrong session, false delivery, stuck composer, cross-talk, or unexplained transcript/DOM disagreement occurs.

NOT RUN — the provider, owned Browser, DEV authentication, fixture grant, or required model budget is unavailable; or a written/uncertain delivery cannot be classified without replay.

## Evidence to record

Record the case ID, surface, provider, UTC time, build SHA, boot ID, workspace/tab/session/binding IDs, marker/send/turn IDs, raw exit, relevant redacted JSON, DOM count, final idle state, and the actual opened screenshot path. Keep host and DEV identities separate. For the recipient extension, record both recipient IDs and both own-session receipts.

## Cleanup

Close only the owned Browser, child processes, tabs, and workspace after saving evidence. Never remove provider session history or foreign fixtures. Preserve any uncertain send for classification.

## History of notable runs/defects

Turing's historical report records single-turn delivery, two CLI sends with Sent-to cards, and timestamp widths as PASS; those reports seed the recipe, not a current release result. Keel's early input/restore records are historical and partially superseded. The comprehensive wave recorded the tested Codex Chat and Claude PTY single-send/reload cell as PASS for task_tfrk57. The identical-send pair cells were explicitly NOT RUN after the frontend identity gate stopped the attempt. Their evidence is in C:/Users/dev/.codeterm/runtime/agent-pads/1ba37774/comprehensive-wave/.
