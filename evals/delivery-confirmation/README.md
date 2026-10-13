# Delivery confirmation drill (real-token, canonical DEV only)

This drill submits ten messages per invocation to a dedicated, already bound
Claude or Codex PTY. Run all eight provider × path × mode groups for 80 sends.
`composer` observes actual visible Chat composer submissions; it never substitutes
an API POST for the UI. `cli` invokes the bench lib's `dc` wrapper. No payload is
retried. Readiness/settlement waits subscribe to daemon events with failure
deadlines, without sleeps or polling intervals.

Run on canonical DEV after merging the lane and verifying its build SHA.
Use dedicated tabs with no other sender. Resolve each tab's exact bound session,
launch nonce and append-only JSONL path; do not choose a latest/global session.
`--caller` is a DEV tab with a private Agent Pad, where the drill writes its
payload files. CLI sends read those pad files through `--from-pad -` with byte
preserving stdin and no source-tab identity, exercising the user CLI ledger path.
The caller and targets must belong to the same DEV instance.
The drill verifies target provider, PTY backing, and acknowledged session nonce.

```bash
bash evals/delivery-confirmation/run.sh \
  --provider claude --tab CLAUDE_TAB --session CLAIMED_SESSION \
  --transcript /absolute/path/to/CLAIMED_SESSION.jsonl \
  --caller DEV_CALLER_TAB --path cli --mode idle
```

Repeat for both providers, both `--path composer|cli`, and both `--mode idle|busy`.
For `composer`, submit each printed payload exactly once in the target's visible
Chat composer using the Domios Browser or manual DEV inspection; preserve the
entire printed body. The runner observes the new ledger receipt and advances.
For `idle`, every send starts from authoritative idle state. For `busy`, first
start a substantive tool-using turn on the dedicated target; every send must
start while authoritative status is working. An agent finishing before ten busy
sends invalidates the group and fails loudly. It is never silently relabeled idle.
Use event-authorized work/finish signals, never fixed-delay warmups.

Each result includes the send ID, ledger state, provider, path, mode, bound
session/nonce, send-time byte anchor, exact full payload, hash, matching record
UUID, record byte offset, JSON field path and complete matching own-session
record. Matching only examines complete records appended after the anchor.
A truncated payload or foreign declared session cannot supply evidence.
The daemon's ledger is authoritative; a transcript match never fabricates a
`delivered` state. Pending/written/absent at a failure deadline is recorded as
such and makes the run fail. Group summaries distinguish received-but-unconfirmed
sends and report confirmed/total. All ten must be delivered with own-session
evidence for exit 0. Files land in `target-check/evals/delivery-confirmation/`.

The model tests are source-only fixtures, not live-provider confirmation rates:

```bash
python3 -m unittest discover -s evals/delivery-confirmation -p 'test_*.py' -v
bash -n evals/delivery-confirmation/run.sh
```

Canonical Rust selectors (one shared app compile, focused executions):

```bash
./build.sh test-one app service::chat_send_evidence::
./build.sh test-one app service::chat_send_transaction::
./build.sh test-one app service::pty_io::submit_transaction::
```

The captured owner baseline is already committed in
`plugins/provider-claude/tests/fixtures/owner-mid-turn-2.1.296.jsonl`:
two consumed `queued_command` attachments, one exact standalone payload and one
owner payload glued directly to a routed report. Whole-record equality matches
1/2; full-payload containment matches 2/2. This is a deterministic evidence
comparison, not an executed live ledger confirmation rate. The Rust regressions
use these original records, reject truncated/foreign/pre-anchor evidence and
preserve delivery against a late transport classification.

A stale pending record can settle from its own correlated ledger receipt:
written/unconfirmed becomes explicit uncertainty, and exact delivery becomes
accepted cleanup. Retained input never blocks restore, and historical pending
input becomes uncertainty without joining the launch payload. A record whose historical ledger receipt and append anchor were
lost cannot safely be classified from a digest plus current-session activity.
The legacy owner record resumes via this uncertainty path. Durable session,
nonce, append-anchor and write-witness fields plus a visible retained-input Chat
bubble are still a separate implementation slice.
