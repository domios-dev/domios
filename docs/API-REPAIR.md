# Tab Identity Repair

REST contract for `POST /api/tabs/{id}/repair`, the verb behind `codeterm tab repair`.

`codeterm tab repair` corrects a live PTY tab without restarting it. An empty
request or `{"from_evidence":true}` verifies the tab's bound session and unique
provider record, then corrects the provider label and clears its parser lock.
`{"provider":"codex"}` adds an assertion that must match that evidence;
`{"session_id":"..."}` consumes verified rebind; and
`{"from_journal":true}` walks lifecycle checkpoints newest-first until provider
and session match the live evidence. It reports the restored checkpoint and all
newer records skipped with reasons. Parser-origin checkpoints are never sources.

The caller is proven from the tab and binding headers injected into a CodeTerm
PTY, plus the session header when the launch already knew it. Only the target
tab's own agent or the live owner of its bound Plan task may repair it. Refusals
use stable `tab_repair_*` error codes, including
`tab_repair_verified_session_required` and `tab_repair_not_authorized`. Every
accepted identity write is journaled with origin `repair`, previous/new provider,
caller tab, session id, and the verification facts used.


`POST /api/tabs/{id}/repair` reports `provider`, `session`, and `parser_lock`
as `{status, previous, current}`, where `status` is `kept`, `corrected`, or
`could_not_verify`. Its `evidence` names the session/provider plus separate
session-record and binding-nonce verification facts. Journal mode also returns `journal.restored_from` and every
candidate skipped with its reason. An accepted write records an
`agent_type_write` lifecycle transition with origin `repair`, previous/new and
attempted provider, caller tab, session id, nonce-verification fact, and
`applied=true`. Parser-origin entries and refused writes are never restore
sources. Missing session proof is refused with
`tab_repair_verified_session_required`; a caller outside the self/plan-owner
authority set is refused with `tab_repair_not_authorized`.

<!-- revision: 1 -->
