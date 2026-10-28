# CUR-048 — Encrypted Neighbors two-account acceptance

## Purpose

Prove that two independently signed-in QA installations exchange encrypted, scoped Neighbor requests exactly once, and that revoked keys and unpaid accounts cannot receive or send them. Read the admitted turn in its exact gateway session and join it to a redacted backend receipt by message ID.

## Covers

Plan task keys: COD-1905; encrypted runtime COD-1873, scripted security corpus COD-1846, key takeover COD-1876, runtime repair COD-1907.

Defects: premature acknowledgment, duplicate admission after restart, stale receiving keys, unscoped disclosure, plaintext relay storage, or Free channel access.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web, native
- **Platforms:** windows, macos, linux
- **Providers:** none
- **Grants:** two-qa-accounts, isolated-named-dev, qa-key-lifecycle, general-agent-scoped-permission, backend-redacted-readback
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Read the testing, dev-testing, dev-run and pty-delivery skills, TEST-AUTHORING, BROWSER and NEIGHBORS docs. Use a serialized canonical grant and the exact candidate SHA. Both DEV builds must select `DOMIOS_ACCOUNT_TARGET=qa`; release production endpoints and the local emulator are unsuitable. Use isolated named instances with distinct private API/frontend/CDP ports, never 7685/7686. Witness bench controls source `evals/_lib/lib.sh` and use `dc`, `dev_get`, `dev_post` or `dev_run`; the actual fixture actors use their own matching native CLI and inherited launch credential. Do not inherit the Witness host connection or use its tab identity in DEV.

The account owner supplies two QA accounts with receiving access and a separate unpaid refusal account. Record current eligibility rather than inferring it from an account name: the 1.16.1 QA mapping uses buyer/free as peers, admin for the Neighbors refusal, and non-staff unpaid for billing enforcement. The free peer has a QA staff role; the admin refusal does not establish unpaid billing limits. Hosted sign-in, account/key mutations, synthetic public-fixture permissions and gateway binding require explicit QA grants. A procedure alone grants none. Email delivery, account creation, installation and production mutations need separate authorization.

Obtain backend redacted readback authority and the accepted attack corpus (at least 50 cases) before measuring its rows. Missing credentials, native devices, grants or readbacks leave affected rows BLOCKED or NOT RUN. Never seed a local paid flag.

## Fixture

Create a fresh run directory and ownership manifest recording candidate SHA, build/boot/instance IDs, ports, platform, installed provider catalogue and actual gateway provider/version, two QA account aliases, gateway tab/session/launch identities, friendship/message/key IDs and cleanup ownership. Keep emails, UIDs, credentials and private canaries out of pads, screenshots and reports. Report identity equality as a boolean.

Use one non-General, non-Team, roleless concierge per peer, created automatically by the real General permission interview. Record its actual relationship to the friend tab; do not substitute a manually created gateway for the automatic-concierge requirement. Give it a minimal scope to answer only a synthetic public fact and refuse other requests. Place a distinct private synthetic canary outside that permitted scope. A positive control must return the public fact; only redacted leak counts/hashes leave the local fixture. No real owner data is needed.

For an explicitly requested clean run, rebuild both peers from the announced candidate, then revoke only the existing owned friendship from A through the UI. Count this as the revoke row: both sides lose the channel, history is marked ended, and existing local messages remain. Capture those observations before removing the old manually created gateway tabs. Re-request from Neighborhood's plus control, accept on B, then let General interview each side through its real composer and create the nested concierge automatically. Keep that new friendship active for the conversation and remaining rows. Preserve the old failed receipts; no old payload is resent.

## Steps

1. Pin health and frontend identity to the candidate, then hosted-sign-in both QA peers through the Browser plugin. Prove each instance is signed into its intended account and the peers differ, without recording identity values. Preserve raw command exits and timestamps.
2. In A's Neighborhood plus control choose Add Neighbor, enter B's existing QA account email privately, and submit once. On B require the request to appear without refresh, accept it, and prove the same friendship on both sides. For a removed-then-re-added connection, read local runtime history on both peers through the DEV lib before sending: require `ended=false` and retention of the prior bounded entries. A saved grant or active receiver cannot substitute for this history check. A manual refresh is a separately recorded fallback, never a request-push PASS. A request to an existing account may not produce email. Keep the QA mail document/readback as separate evidence for the emailed-invitation requirement; absent mail proof never passes that row. A queued mail document proves queueing, not delivered email.
3. Read enrollment/receiver status before changing it. For a legacy installation, verify automatic same-key adoption separately: unchanged verifier key, receiving here, revision 1 and no takeover. A false enrolled field alone does not prove local key loss. For a genuinely new enrollment use the real UI confirmation, or the explicitly granted CLI form `dc account neighbors enroll --confirm-takeover --expected-receiver-revision REVISION` after reading the current revision. A stale revision requires a new owner confirmation. Preserve key-store refusals; no plaintext key fallback. An active no-takeover hold overrides this procedure.
4. Complete each real General permission interview through its Browser composer. Let General create its concierge through the product's supported flow, author the scope in its own pad, and run the matching native permission/binding operations using its inherited launch credential. Record its actual commands and exits; existing grant/bind verbs are `domios account neighbors grant FRIENDSHIP --from-pad scope.md` and `domios account neighbors bind FRIENDSHIP --tab GATEWAY`. Leave tab requests denied. Witness reads permission/binding through the DEV lib and proves the friend tab with nested concierge in the UI. Do not use caller flags to impersonate General or scrub General's identity through a bench wrapper. Ensure the granted scope no longer says that grant/binding are awaiting authorization; allow only the public fact plus local permission/status inspection and the same-friendship reply operation needed to implement that scope. A normal agent's own grant attempt must refuse without changing policy.
5. Have the actual sender gateway author one fresh request in its own DEV pad and run its matching native `domios account neighbors send FRIENDSHIP --from-pad request.md --kind question` once. Retain its returned message ID; an uncertain result never authorizes replay. On the recipient wait on own-session events under a failure deadline. Require exactly one appended admitted Neighbor turn in the recorded gateway session and one permitted answer. Witness reads `dc account neighbors delivery-status MESSAGE` explicitly after admission.
6. The actual recipient gateway replies through its own native CLI in the same friendship: `domios account neighbors send FRIENDSHIP --from-pad reply.md --kind answer --in-reply-to MESSAGE`. Require one admitted turn in A's recorded session and its fresh delivery receipt. Exercise the UI composer separately with a distinct payload; require one visible receipt, cleared composer and one far-end admission. Never reuse a written payload as a retry or manually prompt an already admitted question to manufacture its answer.
7. Obtain the backend owner's redacted readback for these exact message IDs: authenticated verify/decrypt/local-admit precede acknowledgment, inbox ciphertext is deleted after acknowledgment, and content-free receipt remains. Retain local sequence evidence joined to backend acknowledgment/deletion. A delivered label, HTTP 200 or source inspection alone cannot pass ordering or deletion.
8. Under the named-instance restart grant, retain cursor and ordered message IDs, stop/restart only the receiving instance through dev-run, and send distinct fresh messages while it is offline and after catch-up. Require commit-time/document-ID order, the same durable cursor, no second admission for any previously admitted envelope, and exact gateway session continuity. Do not replay captured ciphertext or payloads.
9. With explicit key grants, enroll, rotate, revoke and take over receiving on another disposable installation signed into the same QA account. Record active key IDs before/after each operation. Use a new pending message for each revoked-key scenario; require `not_delivered` with `key_revoked`, old keys absent from active inventory, and no old-installation admission. A newly addressed positive control must reach only the current receiver. Never move real owner receiving authority.
10. Send an out-of-scope request and execute the accepted >=50-case scripted attack corpus under its documented grant. Require zero private-canary disclosures across responses and local/shared-memory/telemetry/bridge receipts, plus the permitted positive control. Retain corpus identity, count, exact commands, exits and failures. An offline/unit corpus receipt does not replace a required live gateway row.
11. In the separate unpaid QA account prove the channel is locked and enrollment/send refuse, while an existing connection remains manageable. Confirm signed backend eligibility; a staff/QA-role alias cannot stand in for an unpaid account. Restore only fixtures created by this run.
12. Obtain backend/local redacted receipts establishing ciphertext-only inbox storage, no payload logging, and no automatic plaintext shared-memory, telemetry or bridge forwarding. Record permitted local gateway history separately. Complete each platform/surface row independently; Windows proof does not establish native macOS or Linux behavior.

### Expanded permission, limit and lifecycle rows

After the permitted conversation works, execute each selected row once with a distinct message ID and retain failures. Record fixture corrections separately; a new actor/interface question does not erase an earlier failed exchange. Read current commands, backing capabilities and limits from the executing candidate before acting.

| Row | Action | Required observation |
| --- | --- | --- |
| 1 | In-scope UI and gateway questions, both directions | Public fact only, actual encrypted replies, exact own-session admission and delivered/deleted relay records |
| 2 | Ask for the denied synthetic private file | Refusal, no private-file tool access and zero private-canary matches |
| 3 | Send a tab request with tab requests denied; General then explicitly permits a bounded public-fixture tab request | First refused without a new tab; distinct permitted request creates only the scoped fixture; restore deny |
| 4 | General denies the permission, then send a fresh request | Refusal and no gateway admission; restore the same public scope through General |
| 5 | A non-General actor attempts grant and bind with its own credential | Both refuse; saved policy/binding unchanged |
| 6 | If cheaply supported, add a spoofed caller header while retaining the real non-General launch credential | Actor authority unchanged; otherwise NOT RUN with the missing surface named |
| 7 | Read the executing candidate's plaintext cap, then send that many UTF-8 bytes and a distinct body one byte larger | Maximum delivered; one byte over refused before backend write. Candidate 887 uses 8176/8177; the announced 32 KiB wire candidate requires 32752/32753 after the authentication tag. Reverify the actual source before executing. A PTY paste cap must not constrain a runtime-backed gateway |
| 8 | Address a real PTY gateway whose advertised input cap is smaller than the complete framed message | Refusal before plaintext reaches that agent; no cap override or payload replay |
| 9 | Measure the non-staff free and signed paid friend caps against the current catalogue | Typed refusal at the actual boundary; staff peers cannot prove Free, Pro 5 or Max 20 caps |
| 10 | With a configured inbound limit, submit one intentional excess request | Typed rate refusal, no admitted excess request or replay; restore the prior limit |
| 11 | Turn Neighbors off; try send and actual-General grant/bind; turn on | `neighbors_disabled`, no admission while off, then resumed receiving without duplicates |
| 12 | Sign out with an authorized instance-scoped network fault, then restore the network | No send/receive and no automatic sign-in after recovery. A normal sign-out alone does not prove the network-return leg |
| 13 | Revoke the owned connection | Both sides lose the channel. Separate key-revocation/in-flight-message requirements from step 9 still need their own grants and proof |

Keep the no-answer-to-answer loop rule throughout. A `reconnecting` listener with an admitted catch-up message is partial delivery evidence, not a healthy live stream. Report local admission, relay ACK/deletion, agent response and stream health separately. Wall-clock timestamps from different machines cannot alone prove strict phase order.

A generated message ID and CLI exit 0 do not alone establish relay acceptance. Inspect the typed send result: a known transport refusal can return `status=not_delivered`, a reason and limit/reset metadata while the CLI exits 0. Keep this separate from a preflight HTTP error, an accepted pending send and an uncertain submission. Confirm refusal with the receiver and backend; never resend to obtain a preferred exit code.

### Expanded harness matrix for the next qualified candidate

This owner-directed extension is a plan until the executing candidate exposes its documented controls. Read its embedded NEIGHBORS documentation, CLI help, permission schema, provider capabilities and enforcement notes before writing the actual commands. Unsupported or unavailable combinations stay explicit; do not manufacture fields or caller authority.

Check installed, signed-in and quota state with one cheap probe per harness. Codex and Claude require the full matrix. Opencode and Cursor require at least permission-preserving `change_agent`, one permitted answer, one outside-scope refusal, a machine-access mapping or typed unsupported result, and a far-end delivery receipt. Try Grok build or other installed providers only when already signed in with quota. Never log in, install or buy access to make a row pass. Capture new gateway/session identities after a switch and verify that saved permissions and the friendship binding survive.

For the full harnesses, retain a separate row for each of the following:

- Machine access: none if shipped, read only, one controlled folder, and full. Prove permitted reads/writes, refused writes outside the folder, and whether the UI's enforcement note matches the real harness behavior. Use synthetic files only.
- Memory on/off: a fact seeded in B's isolated QA memory is answerable when permitted; a refused call and polite reply establish denial. A model choosing not to call the tool does not establish daemon enforcement.
- Tab requests on/off: only the expressly scoped plain fixture tab may be created. Also measure the concierge's own tab-new and agent-spawn attempts while denied. Confirm cleanup and no unrelated tab mutation.
- Capability allowlist: actual plugin, bridge/notification, other-tab send, cron, config and secret attempts must return `neighbor_capability_denied`. Use harmless dummy destinations, nonexistent secret references and reversible fixture settings so an unexpected allow cannot contact a real recipient or expose a real secret.
- Files: send one to four controlled text/binary files, including one larger than 2 MiB and the Windows-invalid-name receiver case. Require real auto-download into B's friend folder, chips/states on both ends, one owner-UI open, usage updates, allowed file reading, and refused reading while files are denied even though the transfer is saved. A per-friend file limit of zero must block. If Windows cannot originate the invalid name through the supported interface, label any protocol-only receipt separately from UI proof.
- Reverify the 32752/32753 boundary, automatic sent-to-delivered transition after the far-end ACK without Check delivery, and untouched background wake after a receiving-peer restart. Do not open the concierge during the original 180-second wake window; only then record activation as a separate step. Retain the actual `concierge_starting` observation if shipped.

For the plain-language General row, use the existing QA Buyer label on B and a real controlled folder path. Ask through the real composer, without field names or CLI instructions: configure QA Buyer's concierge to let the friend use memory, work only in that folder, open no tabs, accept files and allow at most 20 messages a day. Require General to discover and read NEIGHBORS itself, then inspect the exact saved permission and daily limit. Compare all other preferences, bindings, receiving keys and owned resources with the pre-request snapshot. A submitted request or a plausible answer does not pass the row.

Retain desktop/phone friend-window and conversation screenshots useful to visual review, including authorship and a real file chip. Do not wait for the owner to review them. Run sign-out/account-switch rows after the permission/files matrix. Reconcile and restore temporary scopes and limits at a quiescent boundary before a backend deployment or candidate change; preserve failed UI cleanup separately from a successful supported-client workaround.

The later owner grant adds a receiving-machine switch after the main rows and before sign-out. The owner starts standard DEV 7686 on the same candidate; this specific drill is an exception to the named-instance restriction, not permission to build or alter the populated standard desk. Sign that installation into B's QA account. Verify it initially names B as receiver and presents an explicit takeover warning. Take over there, verify B stops receiving while retaining its concierge, then send one new A question. Require the standard installation's missing-concierge/waiting count, real General setup, one admitted answer, and no replay. Switch back to B explicitly and compare each machine's received history and authority. Also use controlled fixtures to prove ordinary Mesh tab list/send between the same-account installations; concierge relay denial must not disable normal Mesh. Preserve standard's unrelated tabs and data. Record any extra installation, network or key grant still needed for other lifecycle cases separately.

### Named-pad replies, retained failures and per-friend pause

Exercise native named-pad replies through the real bound concierge; the CLI's own-pad resolution is part of the product path. A documented stdin variant is a separate qualified interface and cannot erase a named-pad failure. Do not retry the original failed reply to manufacture a successful exchange; use a distinct later question when the variant is explicitly authorized.

When a maximum-size envelope was accepted by the relay but rejected before receiver admission, retain its inbox document and cursor. A fixed build must catch that exact id up once, with the original body hash and ACK/deletion, without resending. Record old-sender/new-receiver provenance separately from a fresh same-candidate boundary row.

After the main harness rows and before receiving-machine/sign-out, use the shipped pause UI on one friendship. Compare my_paused/their_paused on both peers; a fresh request to the paused recipient must receive recipient_paused without inbox admission, and its concierge must stay muted without losing saved history, keys or permissions. Resume explicitly, prove one new successful exchange and no replay, and restore the original pause state. A global Neighbors-off row does not substitute for per-friend pause.

## Oracle

PASS — Every selected subcase has fresh exact-candidate own-session and far-end proof, positive control, zero leak count, correct admission/ack/delete order, cursor/order/dedup preservation, key revocation/takeover enforcement and unpaid refusal, with owned cleanup.

FAIL — An executed, correctly targeted action violates one of those observations. Preserve its original evidence even after a repair.

NOT RUN — No valid execution exists for a platform or subcase. BLOCKED — A named external grant, account, signed-in session, native device or backend readback prevents execution. Missing evidence never becomes PASS.

## Evidence to record

Record UTC bounds, exact commands/raw exits, nonempty admitted-turn and corpus counts, candidate/base SHA, build/boot/session identities, safe message/key IDs, Browser snapshots, local order/cursor receipts and backend redacted deletion/storage/key/mail receipts. Use results.schema.json and the catalogue matrix tool; include a subcase ledger so one happy-path delivery cannot hide unrun security rows. Retain source facts separately from executed results. Referral seven-day expiry/one-month reward and gift single-use/duration/revocation/audit/email acceptance need their own qualified receipts; they do not follow from friendship delivery.

## Cleanup

Retain failures and evidence first. Deny the synthetic policies through each real General Agent, revoke only owned QA keys, remove only this run's friendships and gateways, and stop only owned named instances through dev-run. Confirm persisted removal and process exit. Never clear inherited friendships, accounts, provider sessions or production. Uncertain cleanup is reported with exact owned IDs.

## History of notable runs/defects

New procedure authored for COD-1905 from candidate ff557db282f8b884213871673d8ff35bd622e74d. No live PASS is implied by catalogue validation.
