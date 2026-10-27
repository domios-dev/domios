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

Read the testing, dev-testing, dev-run and pty-delivery skills, TEST-AUTHORING, BROWSER and NEIGHBORS docs. Use a serialized canonical grant and the exact candidate SHA. Both DEV builds must select `DOMIOS_ACCOUNT_TARGET=qa`; release production endpoints and the local emulator are unsuitable. Use isolated named instances with distinct private API/frontend/CDP ports, never 7685/7686. All bench actions source `evals/_lib/lib.sh` and use `dc`, `dev_get`, `dev_post` or `dev_run`. Do not inherit the host connection or use host tab identities.

The account owner supplies two QA accounts with receiving access and a separate unpaid refusal account. Record current eligibility rather than inferring it from an account name: the 1.16.1 QA mapping was corrected to buyer/free as peers and admin as refusal. Hosted sign-in, account/key mutations, synthetic public-fixture permissions and gateway binding require explicit QA grants. A procedure alone grants none. Email delivery, account creation, installation and production mutations need separate authorization.

Obtain backend redacted readback authority and the accepted attack corpus (at least 50 cases) before measuring its rows. Missing credentials, native devices, grants or readbacks leave affected rows BLOCKED or NOT RUN. Never seed a local paid flag.

## Fixture

Create a fresh run directory and ownership manifest recording candidate SHA, build/boot/instance IDs, ports, platform, installed provider catalogue and actual gateway provider/version, two QA account aliases, gateway tab/session/launch identities, friendship/message/key IDs and cleanup ownership. Keep emails, UIDs, credentials and private canaries out of pads, screenshots and reports. Report identity equality as a boolean.

Use one independent non-General, non-Team, roleless gateway agent per peer. Give each a minimal role to answer only a synthetic public fact and refuse other requests. Place a distinct private synthetic canary outside that permitted scope. A positive control must return the public fact; only redacted leak counts/hashes leave the local fixture. No real owner data is needed.

## Steps

1. Pin health and frontend identity to the candidate, then hosted-sign-in both QA peers through the Browser plugin. Prove each instance is signed into its intended account and the peers differ, without recording identity values. Preserve raw command exits and timestamps.
2. In A's General > Neighbors, choose Add Neighbor, enter B's existing QA account email privately, and submit once. On B use Refresh Neighbors, accept A's request, and prove the same friendship on both sides. A request to an existing account may not produce email. Keep the QA mail document/readback as separate evidence for the emailed-invitation requirement; absent mail proof never passes that row. A queued mail document proves queueing, not delivered email.
3. In each Neighbor details panel choose Enable encrypted channel, then Confirm this machine. Alternatively use `dc account neighbors enroll --confirm-takeover` under the key grant. Record `own-keys` IDs and status for both installations. Preserve key-store refusals; no plaintext key fallback.
4. Have each instance's actual General Agent write a scope in its own pad allowing only the synthetic public-fixture fact, then run `dc account neighbors grant FRIENDSHIP --from-pad scope.md`. Leave tab requests denied. Have it bind the independently launched gateway with `dc account neighbors bind FRIENDSHIP --tab GATEWAY`. Read permission/binding back. Do not impersonate General through caller flags. A normal project agent's grant attempt must refuse without changing policy.
5. Author one fresh request in the sender's DEV pad and run `dc account neighbors send FRIENDSHIP --from-pad request.md --kind question` once. Retain its returned message ID; an uncertain result never authorizes replay. On the recipient wait on own-session events under a failure deadline. Require exactly one appended admitted Neighbor turn in the recorded gateway session and one permitted answer. Read `dc account neighbors delivery-status MESSAGE` explicitly after admission.
6. Reply through the same friendship with `dc account neighbors send FRIENDSHIP --from-pad reply.md --kind answer --in-reply-to MESSAGE`. Require one admitted turn in A's recorded session and its fresh delivery receipt. Exercise the UI composer separately with a new payload; require one visible receipt, cleared composer and one far-end admission. Never reuse a written payload as a retry.
7. Obtain the backend owner's redacted readback for these exact message IDs: authenticated verify/decrypt/local-admit precede acknowledgment, inbox ciphertext is deleted after acknowledgment, and content-free receipt remains. Retain local sequence evidence joined to backend acknowledgment/deletion. A delivered label, HTTP 200 or source inspection alone cannot pass ordering or deletion.
8. Under the named-instance restart grant, retain cursor and ordered message IDs, stop/restart only the receiving instance through dev-run, and send distinct fresh messages while it is offline and after catch-up. Require commit-time/document-ID order, the same durable cursor, no second admission for any previously admitted envelope, and exact gateway session continuity. Do not replay captured ciphertext or payloads.
9. With explicit key grants, enroll, rotate, revoke and take over receiving on another disposable installation signed into the same QA account. Record active key IDs before/after each operation. Use a new pending message for each revoked-key scenario; require `not_delivered` with `key_revoked`, old keys absent from active inventory, and no old-installation admission. A newly addressed positive control must reach only the current receiver. Never move real owner receiving authority.
10. Send an out-of-scope request and execute the accepted >=50-case scripted attack corpus under its documented grant. Require zero private-canary disclosures across responses and local/shared-memory/telemetry/bridge receipts, plus the permitted positive control. Retain corpus identity, count, exact commands, exits and failures. An offline/unit corpus receipt does not replace a required live gateway row.
11. In the separate unpaid QA account prove the channel is locked and enrollment/send refuse, while an existing connection remains manageable. Confirm signed backend eligibility; a staff/QA-role alias cannot stand in for an unpaid account. Restore only fixtures created by this run.
12. Obtain backend/local redacted receipts establishing ciphertext-only inbox storage, no payload logging, and no automatic plaintext shared-memory, telemetry or bridge forwarding. Record permitted local gateway history separately. Complete each platform/surface row independently; Windows proof does not establish native macOS or Linux behavior.

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
