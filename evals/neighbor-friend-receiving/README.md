# Per-friend receiving live scenario

Opt-in, real QA backend and live concierge agents. Never a unit test or an
automatic release gate. Follow [qa-peers](../../.agents/skills/qa-peers/SKILL.md)
for candidate qualification, ownership and account grants first. This bench
does not build, stage artifacts, sign in, create friendships, grant permissions,
copy setups, rotate/revoke keys or start a missing peer. It spends provider
tokens for three fresh public-marker questions and cold-restarts C once.

## Prerequisites

An owner grants this exact scenario and exclusive use of the four named DEV
peers. Run from the clean, pinned candidate checkout in its persistent Team DEV
owner shell (no utility timeout). All four peers must already be running with
`DOMIOS_ACCOUNT_TARGET=qa` and `CODETERM_DEV_WATCH=0`, matching candidate SHA,
native readiness and provenance as qualified by qa-peers. The C prebuilt
artifacts must remain intact for the supported restart. Node with native fetch,
Python 3, Git and Git Bash/bash are required; Windows uses the standard Git Bash
installation. No Browser interaction or screen text is used.

| Role | Named instance | API / frontend / CDP | QA identity |
| --- | --- | --- | --- |
| A | acceptance-r161 | 7707 / 15007 / 9407 | Buyer, sender for X |
| B | curated | 7697 / 14997 / 9397 | Peer, initial account receiver |
| C | curated-second | 7717 / 15017 / 9417 | The same Peer account |
| D | curated-third | 7727 / 15027 / 9427 | Third, sender for Y |

X (A↔Peer) and Y (D↔Peer) must be distinct, active, unpaused friendships. The
second relationship is mandatory; an absent D/Y never becomes an empty passing
assertion. B initially receives both via the account default and has two ready
concierges. C has its **own** enabled local grant and dormant provider/model
setup for X, with no live X concierge. Provision these fixtures through normal
owner/General operations before the bench; nothing is copied from B. A and D
must be effective receivers for their own side of the respective friendship so
the ordinary send authority check permits the questions.

The manifest's `control_settled:true` is a fresh owner acknowledgement that no
preserved offline-message/revoked-key control requires C to remain stopped.
Do not set it merely to launch this bench. In particular, the preserved
`153cc71b` control from the earlier move-back failure must be settled under its
separate grant before C is started by its DEV owner. This bench never performs
that recovery and refuses if C is absent.

## Invocation

Copy [manifest.example.json](manifest.example.json) to a private AgentPad and
replace every placeholder. Pin the full candidate and deployed QA functions
SHAs, authorized QA project, two friendship IDs, and SHA-256 of each intended
QA account UID. Obtain these hashes privately from the authorized QA account
readback, not a guessed label. B/C hashes must agree; A/B/D must differ. Set
`control_settled:true` only after owner clearance. The example intentionally
refuses until filled.

```bash
DOMIOS_LIVE_PER_FRIEND=1 bash evals/neighbor-friend-receiving/run.sh \
  --manifest /c/Users/User/.domios/runtime/agent-pads/OWNER/per-friend-private.json \
  --out target-check/evals/per-friend-CANDIDATE-RUN
```

The deployed desktop must expose authenticated local `GET /api/account/backend`.
Missing diagnostics, unknown schemas, production/emulator targets, a different
actual project or a non-QA functions endpoint kind refuse before mutations.
This is an actual executing-daemon observation, not a manifest or environment
assertion. Setting `DOMIOS_ACCOUNT_TARGET=qa` in the launcher cannot turn a
production-target artifact into a QA-target artifact.

The output directory must be new, directly beneath `target-check/evals` or an
already existing child. Reusing an output directory refuses; a failed send or
uncertain request is never replayed. `--help` and missing opt-in perform no
peer action. Credentials resolve exclusively through `_lib/lib.sh` helpers:
the driver runs under `dev_run`, per-peer `_lib/connection.mjs` delegates to
`dev_run`, and restart uses `dev_assert_target`, `dev_assert_restart_target`
and `dev_run` around named `build.sh kill` / `dev-prebuilt --wait`. No raw CLI,
manual credential read, image-name kill, default DEV or production target is
available. Instance names and all ports are fixed to the table.

## Structural assertions

1. Qualify all four exact candidate/boot/instance/frontend/CDP identities and
   signed-in Firebase identities before any mutation. UID hashes must match
   the pre-authorized QA fixtures. `GET /api/account/backend` must report
   `{schema_version:1,target:"qa",project_id:"domios-signin-qa",functions_endpoint_kind:"qa"}`
   between two same-boot health reads. All four peers are requalified before
   every set, reset, send and shutdown. Only the deployed functions SHA remains
   an explicitly unverified operator attestation in `declared_qa_backend`;
   actual backend target/project/kind are recorded separately with each proof.
2. Require account receiver B, both effective friend states sourced from
   `account`, B's ready bound roots and C's dormant X setup.
3. Submit exactly one `set_friend_receiver` on C for X, using X's current
   effective revision and explicit confirmation. Require `source:friend`,
   receiving on C, X's bound ready root only on C, and no X authority tabs,
   descendants or binding on B. The old X root/descendant IDs must also be
   absent even if their authority fields were cleared. Y retains B's exact tab/session/nonce and its
   account source; the account receiver revision must remain unchanged.
4. Send one unique X question from A and one Y question from D. Match exact
   returned message IDs, bodies and post-arm timestamps in the target history;
   require concierge pickup (`working/answered/refused/no_reply`) and accepted,
   delivered outgoing history. Nonreceiver history must lack each exact ID.
5. Submit one reset on C using X's current effective revision. Require account
   B again, a fresh B X session and no C X authority. Send one fresh X question
   to B and apply the same history checks.
6. Close C's event observer, stop/start only C through the supported named
   prebuilt flow, require launcher exit 0 and a changed health boot ID, then
   requalify its identity. After effective nonreceiver state is known, require
   C's X setup dormant, no binding and no X root/descendants or former live IDs in the full mux
   snapshot. B's Y concierge stays the original session. Recheck all original
   nonreceiver histories once more.

The restart entry point accepts the same private manifest path and C's expected
boot ID, not a candidate SHA plus environment bypass flags. `restart.sh` only
starts `restart.mjs` under `dev_run`; it cannot issue shutdown itself. That
verifier reloads the full manifest, rechecks clean/pinned source and all four
actual backend/account/instance/build identities, verifies X/Y reset on B and
C dormant/nonreceiving, then requalifies immediately before the supported
C-only shutdown/prebuilt commands. Calling either helper directly cannot skip
these checks. A missing manifest, stale boot or missing backend proof refuses.

Authenticated event streams are armed before actions. Reads advance only on
events, with no sleeps, polling, warmups, mutation retries or deadline extension.
Each observation has the manifest's 30–180 second failure deadline. An event
stream loss/overflow is a failure, not a passing negative observation.

These are local host routing, pickup, retirement and persistence oracles. They
do not independently prove native provider transcript appends, credential
revocation or backend inbox deletion. Keep Turner's backend receipts/native
session evidence when those additional acceptance claims are required.

## Results and failure ownership

`results.json` records structural checkpoints, exact synthetic message IDs,
markers, safe history fields, roots/session IDs and hashed binding nonces. It
stores no API token, account UID, email, unrelated history or raw account/tab
snapshot. `C-restart.log` is private launcher evidence. Exit 0 and
`outcome:PASS` require every assertion; failure returns nonzero with its first
cause and the durable intent ledger. Preserve the output as private QA data.
An unmet preflight before any intent is `BLOCKED`, not a tested product failure.

The run leaves all four peers running, the account receiver B, X reset and Y
unchanged; it retains its three test conversations. It does not delete history
or clean up inherited resources. On failure it does **not** automatically reset
an override, restart again, resend a question or adjust grants. The owner
reviews the last intent/outcome and restores only the declared fixture state.
