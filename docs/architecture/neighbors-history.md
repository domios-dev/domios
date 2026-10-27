# Neighbors history and receiving installation

Design proposal for owner and Orchestrator review. This document changes no runtime or backend behavior.

Source baseline: desktop `4ca7a39494c78ebf0ab6ac908e97f7d23f8f27d6` (2026-10-27). Backend implementation and deployed Firestore rules are not present in this lane. Proposed paths below are not claims about existing backend collections. Turner must confirm backend facts in `codeterm-firebase` before implementation.

## Product decision

Keep **one live receiving installation per account**, as selected by the owner. Friendship belongs to the account; gateway agents, local permissions and live delivery belong to the selected installation.

Every signed-in machine shows `Receiving on: <machine name>`. Other machines show the same friendships and explain that messages arrive on that machine. `Receive here` is an explicit, confirmed account-wide switch covering all friends, even when invoked from one friend's panel. Sign-in, startup, opening history, connectivity loss and gateway creation never switch receiving automatically.

Conversation history is separate from delivery. It contains both directions, survives acknowledgement and transport key rotation, and can be read on authorized account machines without creating or feeding agents there. Reading never calls `friendsAck`, advances an inbox cursor or injects historical messages into a gateway. Initially only the receiving installation sends too, matching the current enrollment/signing-key requirement. Secondary machines display history, destination and switch action.

For 1.16.1, ship explicit receiver identity and lifecycle correctness if their backend proof is ready. Defer cloud history and archive-key provisioning to a dedicated release. The current delivery receipt list is not a conversation archive.

## Current architecture: evidence and limits

Paths are relative to the repository root. `N` below means `src-tauri/crates/codeterm-neighbors/src`; `S` means `src-tauri/crates/codeterm-service-core/src/service`.

| Source | Observed behavior and implication |
| --- | --- |
| `N/adapter/firestore.rs`: `listen`, `Listener::query`, `next`, `parse_document` | Bearer-authenticated native Firestore gRPC reads `friendInbox/{uid}/messages`, filters `envelope.friendship_id`, orders ascending by `received_at`, then document name, and starts strictly after the cursor. Catch-up uses RunQuery pages of 100, followed by Listen. CURRENT marks readiness, including an empty inbox. Buffers are bounded at 500 changes; decoding at 2 MiB. Parsed documents have exactly `envelope`, `signature`, `received_at`, `expiresAt`. This is already event-driven transport. |
| `N/adapter.rs`: `FirebaseTransport`, `FirebaseRegistry` | `friendsSend`, `friendsAck`, `friendsDeliveryStatus`, `friendsPeerKeys`, `friendsEnrollVerifierKey`, `friendsRevokeVerifierKey`, `friendsOwnKeys` are callables. Enrollment carries public signing/encryption keys and optionally `replace_others`; this adapter does not send an installation id. Own-key decoding consumes `key_id` only. |
| `N/wire.rs`, `N/crypto.rs`, `codeterm-neighbors/test/golden-v2.json` | V2 signs canonical envelopes with Ed25519. Static X25519 plus HKDF-SHA256 derives AES-256-GCM encryption using friendship and ordered sender/recipient encryption-key ids. Signature binds identities, kind, reply id, ciphertext, nonce, scope hash and keys. Ciphertext maximum is 8,192 decoded bytes; plaintext maximum is 8,176 UTF-8 bytes. The fixture proves a wire expectation, not deployed backend/rules. |
| `N/receive.rs`: `Receiver::process`, `catch_up`, `verify_inbound` | Checks document id `<sender_uid>_<message_id>`, friendship/recipient, signature and key binding, expiry, then decrypts, admits locally, acknowledges and saves cursor. Cursor deduplicates by commit timestamp plus id. Permanent invalid input can advance the cursor; missing local key, revoked key and transport/store failures stop progress. Submit uncertainty never authorizes replay. |
| `N/keys.rs`: `InstallationKeys`, `KeyScope`, `RecipientKeys` | Private keys are scoped by installation plus UID, in current/candidate slots. Enrollment requires takeover confirmation. Reconcile promotes an accepted candidate, revokes other active keys, verifies one remaining key and removes candidate. Rotation overwrites current: there is no archive of old decryption keys. Decrypt requires an active verifier id. Revoke gathers all active ids, revokes them and deletes current/candidate slots. |
| `N/keys/os.rs` | Neighbor secrets use Windows Credential Manager, macOS Keychain or Linux Secret Service. They are separate from account Mesh possession keys and refresh credentials. |
| `S/neighbors/runtime.rs` | Send requires local GA-authored permission and active local signing key, chooses newest peer key by `created_at`, encrypts and sends. Transport uncertainty returns a message id with pending/unconfirmed submission; no body archive is written. Status returns local enrollment, bindings and listeners, without authoritative remote machine identity. Listeners start for valid local bindings and paid channel access. |
| `S/neighbors/storage.rs`, `S/neighbors/runtime/gateway.rs` | Owner-only UID-scoped files hold settings, per-friend cursors and gateway admission receipts. Settings bind friendship to a local independent agent tab and store GA permissions. Admission receipts record claimed/pending/delivered/uncertain state to prevent unsafe replay; they are not message bodies or account history. |
| `S/neighbors/runtime/listener.rs`, `N/supervisor.rs` | Listener sessions are generation-fenced and check channel access before receiving batches. Retry is explicitly event-triggered. History should reuse transport/authentication infrastructure without entering this receiver state machine. |
| `src/components/Neighbors/{useNeighborChannel.ts,NeighborChannel.tsx,neighborRuntimeClient.ts,NeighborsView.tsx}` | Panel stores at most 20 outgoing content-free receipts in UID/friendship-scoped localStorage. It confirms takeover and warns rotation loses waiting messages. There is no body-history query, incoming timeline or authoritative remote machine field. Gateway/sidebar navigation uses local tab ids. |
| `S/neighbors.rs` | Friendship operations proxy `friendsList`, `friendsRequest`, `friendsRespond`, `friendsRemove`, `friendsSetLimit`. The desktop does not establish backend collection layout. |
| `src-tauri/crates/codeterm-account/src/account_mesh/{wire.rs,key.rs,registry_keys.rs,registry_presence.rs}`, `S/account_mesh/presence.rs` | Mesh has installation identity, Ed25519 possession proof, key generations, slot/registry/metadata revisions, display names and signed-in/signed-out presence. Notices cause authoritative refresh; notices themselves are not authority. Same-account Mesh authorization does not supply Neighbor archive decryption. |
| `S/account.rs`: `account_logout_owned`; `S/neighbors/runtime/{logout.rs,callable.rs}` | Logout invalidates Neighbor generations/stops listeners, runs account logout and local Mesh block, then records `cleanup_pending`. Registry cleanup is retried on a later authenticated Neighbor operation. Calls check UID/login generation before and after a callable; Listen captures a token when opened. These local fences do not prove immediate server refusal of a friend's send after logout. |

The stable contract in `essences/core.md` says acknowledgement deletes relay ciphertext and plaintext stays in local gateway/session/delivery storage, outside shared memory, bridges, Mesh replication and diagnostics. Backend deletion implementation is not visible here. Simply retaining inbox documents would omit the sender's copy, depend on discarded keys, retain delivery expiry and mix reading with execution. It is not the history design.

## Target data model

All paths here are **proposed contracts for Turner**. Keep friendship identity where its semantics fit; do not add backward-compatibility shims. No historical backfill is promised: acknowledged ciphertext and overwritten keys cannot be reconstructed.

### Receiver authority

`neighborAccounts/{uid}` is a small server-owned control document:

- Nullable `receiver_installation_id` and `receiver_key_id`; monotonically increasing `receiver_revision`.
- `receiver_session_id` or equivalent installation-bound session generation; `receiving_enabled`; server mutation timestamp.
- Current public archive-key epoch and public encryption key; policy/version metadata. No private keys.

Resolve machine display name from the account machine registry. Installation id is authority; cached name is presentation. Publish a sanitized receiver summary to active friends through an authorized endpoint or friendship-scoped document, never the complete machine inventory. Offline remains selected until explicit switch. Deleted/signed-out receiver shows `Not receiving`, with a last-machine label that does not imply delivery is possible.

Only the backend changes receiver authority, transactionally with key activation/revocation, after fresh installation-bound session and possession proof. Revision compare-and-swap prevents competing switches. A stale caller refetches and requires a fresh explicit decision; it never silently takes over again.

### Owner-readable conversation archive

Use owner timelines, not an unbounded array or one growing conversation document:

```
neighborHistory/{ownerUid}/conversations/{friendshipId}
  members: [uidA, uidB]               # immutable, server-owned
  relationship_epoch, owner_history_generation, closed_at, last_activity_at
  retention_days: 90
  message_count, accounted_bytes
  messages/{senderUid_messageId}
    archive_schema, relationship_epoch, owner_history_generation
    sender_uid, recipient_uid, friendship_id, message_id
    accepted_at                     # server timestamp, ordering authority
    expires_at                      # accepted_at + 90 days, archive TTL
    signed_packet                   # ciphertext, wraps, immutable signed fields
    delivery_status, reason          # server projection, outside signature
    delivered_at                    # nullable; gateway admission, not human read
```

Every accepted message creates one row in **each participant's owner timeline**, with identical signed encrypted packet and server acceptance identity. Each reads their own namespace in both directions, never the other's owner namespace. Backend sees routing/timing/size/status metadata but cannot decrypt content. No plaintext body, preview, content search index or summary goes into Firestore. Sender history is recorded at acceptance, independently of recipient acknowledgement or panel opening.

Before submission, persist the exact encrypted signed packet and message id in a bounded installation-local outbox. This preserves a pending/unconfirmed row through a lost response or restart without retaining a plaintext draft. Reconcile acceptance/status by that id, merge into the accepted history row, and never automatically resend uncertain content. Unaccepted local drafts are not recipient history. Outbox storage shares the encrypted cache bound and refuses new work when full; it cannot evict unresolved submissions silently.

Keep delivery inbox separate. `friendsAck` deletes only delivery work and updates both timeline status projections. It never deletes archive rows. Pending, gateway-delivered and not-delivered do not mean a human read or answered. Views may briefly lag while status projections converge; receipt identity is durable and idempotent.

Proposed explicit bounds: 90-day visible history, at most 10,000 rows per owner/friendship and 256 MiB of accounted archive documents per owner. Preserve the 8,176-byte body limit; bound the complete stored packet at 24 KiB including wraps/signatures. Account for full serialized rows and both directions within each owner's quota. A send exceeding either party's quota refuses atomically with a typed archive-full result; never deliver without history. Expiry/deletion releases capacity through idempotent accounting. This favors an honest hard bound over silent history loss. Turner must confirm accounting and operational quota choices.

TTL deletion is asynchronous. UI hides expired rows; purge workers/monitoring enforce physical cleanup with a deletion SLA Turner must specify. Do not promise erasure at exactly 90 days. Expired ciphertext can remain owner-readable until deletion; strict time-of-day read revocation needs an additional query/rules contract. Manual delete immediately tombstones access to a conversation generation, then purges rows/counters. Reopening uses a new generation so deleted history never revives. Account deletion removes owner rows, packages, control data and retained key records according to backend policy.

An optional local cache is encrypted with an installation-local OS-stored cache key. Bound it to 2,000 rows or 32 MiB per UID, whichever comes first, with asynchronous per-record storage and least-recently-used page eviction. Keep plaintext in bounded active-view memory and existing authorized gateway/session delivery paths. No body in localStorage, shared mem, telemetry, notifications, bridges or generic Mesh replication. Cache failure must not prevent history access.

## Encryption and authorized machines

Transport authority and historical readability need separate lifecycles. Introduce V3 packets and **account archive encryption keys** distinct from installation transport/signing keys. Do not retain V2 live keys indefinitely as a history mechanism.

For each message:

1. Generate a random 256-bit data-encryption key (DEK), encrypt UTF-8 body once with AES-256-GCM and fresh nonce. Authenticate version, relationship epoch, friendship, sender, recipient, message id, kind and reply id as associated data.
2. Wrap DEK to three public keys: sender account archive key, recipient account archive key, and recipient's selected transport key. Use a reviewed standard HPKE suite (X25519/HKDF-SHA256/AES-256-GCM), with explicit domains and recipient-role/key-id context, rather than inventing a wrapping construction. Erase DEK after use.
3. Sign the complete canonical packet with Ed25519, including ciphertext, wraps, ids, scope hash, issuance metadata and expected receiver revision. Backend validates identities, friendship, active sender key, archive epochs, revision and size before acceptance. Receiver verifies before unwrap/decrypt; archive reader verifies the same immutable signature and historical binding.

This is a wire change, not an additive field in V2's deny-unknown-fields structures or four-field inbox parser. Specify canonical encoding and cross-language golden vectors first. Reject missing archive wraps. The server cannot verify a malicious sender wrapped the correct DEK; clients expose corrupt/undecryptable rows honestly, never unauthenticated plaintext. Backend-controlled membership/key directory prevents another account substituting archive keys. This retains the existing backend-trusted key directory; it does not introduce key transparency.

Both owner copies may contain the same packet and public-key wraps. Firestore namespace authorization is still necessary; decryption additionally needs an authorized private archive key. Protection is against a ciphertext relay, not a compromised endpoint. No forward secrecy is promised: retained archive private keys decrypt their epoch's history.

### Additional machine provisioning

Sign-in authorizes friendship metadata, not old-history decryption. Each installation creates a distinct archive-device wrapping key in the OS secret store. A callable registers its public key/session binding. On first setup, an explicit archive-initialization transaction creates the first epoch and authorizes its creating installation; it refuses if an archive already exists. A currently authorized account device explicitly approves subsequent machines and encrypts necessary archive private-key epochs to the target device public key. Backend stores only signed, targeted encrypted packages. Bind approval/package to UID, target installation, key generation, archive epoch and nonce; verify approving-device identity. No plaintext key goes through backend, Mesh, prompts or pads.

Offer an optional owner-held high-entropy recovery secret encrypting the archive key bundle before upload. Recovery requires account authentication plus that secret and a fresh device authorization record. Never derive it from UID, account password, refresh credential or an enumerable PIN. Without an authorized old device or recovery secret, a new machine cannot read old history. It can explicitly reset the archive epoch for future messages and show old rows unavailable; takeover must not silently destroy or claim to recover history.

Encrypted key-package exchange is a **new narrowly scoped capability requiring an explicit reviewed exception to the current machine-local secret contract before shipping**. It changes neither generic secret-store nor Mesh behavior and never transfers transport signing keys, permissions or agent sessions. Without that approved capability, ship history on the original authorized installation only; others say `History locked on this machine`. Do not copy secret files as a shortcut.

### Rotation, revocation and removal

| Event | New traffic | Retained history |
| --- | --- | --- |
| Transport rotation / `Receive here` | Old live key/revision loses send, receive and ack authority. Pending old-key messages become terminal not-delivered/key-revoked; never automatically resend. | Archive keys remain; authorized readers retain both directions, including pending rows with terminal status. |
| Routine archive rotation | New messages require new archive epoch. Stale epoch refuses; only proven unaccepted submissions may be explicitly recreated. | Keep old private epochs in authorized encrypted bundles until last referenced row expires/deletes. Live-key reconcile never erases archive keys. |
| Compromised/revoked device | Revoke installation/session/package access. Disable receiving if selected. Rotate archive key for future messages, distribute only to surviving devices. | No future fetches. Previously copied plaintext, ciphertext and keys cannot be erased remotely; rotation cannot revoke old knowledge. Cloud deletion cannot erase the friend's copy. |
| Unfriend | Close relationship atomically, refuse sends, stop listeners/remove pending work and retire local bindings/permissions. Reconnection uses a new relationship epoch. | Each retains their own read-only history until expiry/deletion. This is proposed product policy: active friendship is not required to read one's owner archive. |
| Sign out | Fence local operations/cancel streams immediately; server-confirmed sign-out disables receiving if bound to that session. No automatic fallback. | Clear active views/decrypted cache keys and this device's usable archive secrets locally; deny server session/package access. Cloud ciphertext remains for authorized devices/recovery. Existing provider transcripts are separate local session history; sign-out does not erase content already shown to an agent. |

Retain immutable server-owned historical public verifier/encryption bindings with UID, key id and acceptance/revocation boundaries, scoped to retained history. Archive verification tests authorization **at server acceptance**, not current activity. It cannot reuse `verify_inbound`, which checks the live peer and expiry. Revocation prevents newly accepted signatures, not verification of old accepted rows. Client `issued_at` cannot prove pre-revocation acceptance; use server-authored acceptance records. Retain public records only while referenced history needs them.

## Sign-out contract and network boundary

Owner rule: after sign-out nobody can message you and you receive nothing. Apply a hard backend admission fence after **server-confirmed receiver-session sign-out**, plus immediate local delivery fence even offline. Secondary nonreceiver sign-out revokes that installation's reads/actions; it does not sign out the still-selected receiver. Account-wide stop is a separate explicit disable action. This preserves current per-machine logout and the selected receiving model.

Bind receiving to a live installation session. `friendsSend` checks sender session and recipient receiver enabled/signed-in/nonrevoked session in the same admission transaction. Logout/session revocation disables that binding, revokes its live key and terminalizes pending work. Do not run today's all-active-keys `InstallationKeys::revoke` from a secondary installation: it could revoke a different machine's new receiver. Cleanup must be conditional on captured installation, session and revision so delayed cleanup cannot undo later takeover.

Locally fence generations first, stop/join listeners and history subscriptions, clear decrypted views, persist cleanup intent, then finish credential deletion. For online logout, commit backend session revocation/receiver disable before claiming global completion. Ack, reads and new streams recheck live authority. Revocation events cancel local work; cached history never bypasses sign-in.

An offline installation cannot inform a remote sender immediately. Key deletion/queued cleanup cannot satisfy literal worldwide refusal at that instant. Show `Signed out locally; server stop not yet confirmed`. Retain a minimal cleanup operation authenticatable without the deleted refresh credential (backend-scoped revocation token or signed installation request), apply on network-restored event. A short server lease can bound uncertainty but cannot make offline sign-out instantaneous; no polling or timer-driven availability proof is proposed. If instantaneous worldwide refusal while disconnected is required, it is unsatisfiable and must not be claimed as shipped. Current source proves local stop plus deferred cleanup intent only.

## Takeover and gateway mapping

1. B reads authoritative receiver/revision. A may be offline without losing selection.
2. `Receive here` confirmation names A, explains account-wide effect and pending loss. B saves a candidate locally before submission, preserving crash-safe enrollment.
3. Backend verifies B's live session/possession; compare-and-swap transaction revokes A's live key, increments revision, activates B and publishes selection. Pending A work becomes not-delivered. Operation id makes retries idempotent; uncertain response reconciles by authority read, never another generated takeover.
4. Signed-in machines observe receiver control through Listen or existing authenticated event/status mechanism if direct control reads cannot be secured. A stops listeners/send controls. B starts only after confirmed authority and valid local bindings. A cannot reclaim on restart.
5. B's GA creates/chooses one independent local gateway per friend and grants local scope. Until then show `Receiving here; gateway setup required`. Queue within existing delivery TTL/bounds without claiming agent delivery. Never replicate A's tab ids, provider session or GA permissions. A's old tab remains a local inactive-receiver transcript.

There is one **active gateway binding per friend on the selected machine**, not one required tab on every machine. Readers need no agent tabs. The common conversation view offers `Open gateway` only for a valid local binding; a remote machine name is not a local tab target. Takeover never replays archive rows or relaunches A's session on B. History can remain locked after successful takeover; show receiver, gateway and history-key states separately.

In-flight admission cannot be recalled: bytes already admitted to A may execute. Revision fencing denies A later ack/reply, cancels unclaimed work and prevents automatic redelivery on B. Archive may honestly show unconfirmed/not-delivered while local execution is uncertain. Seamless handoff would need durable cross-installation execution claims; it is outside this release.

## Firestore rules versus callable history API

Recommend **callable mutations plus direct rules-protected reads**, conditional on proving installation-bound authorization. Keep Firebase credentials in Rust service, outside browser JavaScript. Add a bounded history port in `codeterm-neighbors` and desktop page/subscription operations behind the existing authenticated local API for native/web clients. No second cloud read API is necessary if rules express the whole contract.

Required rules contract for proposed paths:

- Default deny. Client writes to receiver authority, inbox, history, counters, status and verifier records are denied. Callable/Admin mutations validate independently; Admin writes do not rely on rules.
- Owner history get/list requires authenticated UID equal to owner namespace, a live installation-session claim backed by nonrevoked session/device record and a nondeleted conversation generation. Each row's `owner_history_generation` must match the allowed parent generation; queries constrain that field, so reopening cannot expose unpurged rows from a deleted generation. Removed friendship may remain readable under the selected policy. UID-only rules cannot enforce sign-out of one of several machines.
- At most 50 rows per list query; deny global collection-group discovery and other-owner queries. Query bounds do not replace document authorization. Keep dependency lookups constant per query, not new session/friendship checks for every row.
- Historical verifier records are scoped to accepted owner conversations, preferably via a server-owned conversation key directory. Receiver summaries expose only sanitized friend-visible data. Device packages require active target installation/session; approvals and writes require callable possession checks.
- Prove denial of a fresh query with an old unexpired token **and an already-open Listen** when session authorization changes. Token expiry alone does not meet the sign-out rule.

Desktop source does not prove its current bearer tokens carry installation/session identity usable by rules. Turner must confirm claims, session documents, rule lookup/revocation behavior and access-call limits. If not expressible, use bounded authenticated `friendsHistoryPage` callable with equivalent owner/session/deletion checks and an authenticated realtime subscription mechanism for the open view. A page API alone supports on-demand history, not realtime. Never relax rules to UID-only or introduce periodic fetching for convenience.

| Option | Strength | Cost / limitation |
| --- | --- | --- |
| Direct RunQuery + Listen | Existing native channel; realtime deltas; no custom read handler. | Requires installation-aware rules/tokens, bounded indexes/queries and proven revocation. Signature validation and write rate limits remain callable responsibilities. |
| Callable paging | Centralized session/possession checks, bounded validation/audit. | Adds cloud execution/serialization to Firestore reads; separate live event mechanism required. Not inherently cheaper or safer than correct rules. |
| Inbox retained as history | Appears to avoid new storage. | Reject: recipient-only delivery storage, acknowledgement deletion, expiry and current-key dependency do not satisfy history. |
| Transcript scraping / Mesh replication | Plaintext already exists locally. | Reject: incomplete coverage, provider/session coupling, private gateway context leakage and no account retention/access model. |

### Paging, realtime and cost

Open a friend with a descending latest-page query on `(accepted_at, document id)`, limit 50; reverse for display. Older scroll uses a strict two-field cursor, never offsets, fetching at most 50. Publish matching indexes. Server timestamp plus id is authority, not client clock.

For realtime, prefer latest-50 Listen as the initial query, avoiding duplicate first-page RunQuery. Rows leaving the rolling window remain paged history; window removal is not deletion. Deduplicate by owner/friendship/message identity. After reconnect or a burst larger than the window, explicitly catch gaps with bounded cursor pages before joining the live window; newest 50 alone cannot prove completeness. Deletion uses conversation generation/tombstones and explicit page reconciliation. Cancel subscriptions on friend change, panel close or sign-out. Do not subscribe to every friend's full history in the background.

Planning counts, not measured billing or price quotes:

- Open: up to 50 message documents plus a small fixed set of control/session/conversation/key reads. Cached key directories can avoid repeated key reads.
- Older scroll: up to 50 more document reads plus applicable rule dependency/index charges. Reopen/reconnect can incur initial reads again; cache does not imply free server reads.
- Live panel: matching changes including status projections, rolling-window/reconnect effects. Each independently open machine/view adds reads.
- Accepted send: two archive row writes, one delivery item and bounded quota/control/receipt work. Ack/status projection adds writes. Storage is duplicated across owner timelines and packet size bounded.

Turner must price actual billable minimums, index-entry/rule-dependent reads, TTL deletes and reconnect behavior for deployed queries. No dollar estimate or zero-read claim is made. Work is bounded by pages and visible subscriptions, not conversation age or polling frequency.

## Exact backend work for Turner

1. **Receiver/session fence:** extend enrollment/status (`friendsEnrollVerifierKey`, `friendsOwnKeys` or proposed `friendsReceiverStatus`) with installation/session identity, machine summary, expected revision and operation id. Enforce one active receiver transactionally, not via client sequential revoke. Fence send/ack on revision. Add conditional `friendsDisableReceiving`; integrate receiver disable with sign-out/session revocation. Secondary logout cannot revoke another receiver.
2. **Archive directory/provisioning:** current public archive epoch, immutable historical verifier records, device public registration/approval, signed encrypted packages, device/session revocation, optional encrypted recovery bundle and explicit reset. Never accept private archive keys or DEKs in plaintext. Define exact authority/proof and immutable field contracts first.
3. **Atomic V3 acceptance:** `friendsSend` validates relationship epoch, channel entitlements, sender authority/rate/scope constraints, recipient receiver session/revision, archive epochs and packet bounds. One idempotent transaction creates acceptance identity, both history rows, inbox and quota accounting. Same id/same packet returns original acceptance; same id/different packet refuses. Never partially accept without both histories. Publish exact current entitlement policy rather than trusting UI booleans.
4. **Receipt/cleanup:** idempotent `friendsAck`, authorized for current receiver/session/revision, deletes inbox and projects status to existing archive rows. Define terminal status on rotation/takeover/TTL/unfriend. Late ack cannot recreate a deleted timeline. Keep bounded receipt identities/tombstones for in-flight reconciliation, without indefinite body retention.
5. **Read policy/indexes:** default-deny installation-aware rules, bounded owner queries, scoped key directories, sanitized receiver summaries, composite indexes and archive/inbox TTL. Add callable paging only if equivalent direct-read security cannot be established.
6. **Retention/deletion:** transactional quotas, idempotent deletion accounting, purge monitoring, owner conversation-generation deletion fence, account/package deletion. Confirm existing friendship/key-revocation cleanup before replacing assumptions with facts.

Needed confirmations from `codeterm-firebase`: actual collections/rules/indexes; inbox TTL and ack deletion; send transaction/idempotency; verifier history/revocation and atomic replacement; installation-bound claims/session revocation; existing Listen denial after session changes; receipt lifetime; friendship-id reuse; paid/free send/receive policy; account deletion behavior; backend deployment SHA and shared fixture ownership. Desktop fixtures cannot establish these. Implementation dispatch must obtain Turner's backend SHA/contracts before coding against them.

## Release-sized delivery

| Step | Backend prerequisite | Desktop changes | Release boundary |
| --- | --- | --- | --- |
| A. Receiver identity and sign-out | Receiver transaction, installation-bound status/send/ack fences, conditional disable and revocation observation. | Extend adapter/wire/runtime status/enrollment with revision and machine; show destination/explicit switch and distinct offline/no-receiver/missing-gateway states. Preserve local GA grants/bindings. Correct logout ordering/conditional cleanup. Update CLI contracts if arguments change. | **1.16.1 candidate**, only after backend review and focused takeover/logout proof. V2 can remain in this bounded step. No history claim. |
| B. Archive protocol and device keys | Public directory, historical verifier records, approval/recovery contracts and rules. | Separate OS-stored archive/device key lifecycle; V3 canonical encoding, standard HPKE, historical versus live verification. Cross-language golden vectors; stable contract update for explicit encrypted key provisioning. | Later release, protocol/security review first. No V2 retention workaround or raw secret export. |
| C. Atomic archive and timeline | V3 transaction, two owner timelines, receipt projection, quotas, TTL/deletion, rules/indexes. | History port/local API, bounded paging/Listen, stable row/status merge, gap catch-up/cancellation. Shared Neighborhood/friend panel shows both directions, locked/unavailable keys and retention. Optional encrypted cache cannot block startup. | B+C together form first real history release after end-to-end proof. Breaking cutover accepted; no old-message migration/backfill. |
| D. Other-machine history | Approved packages/recovery and device revocation fully deployed. | Approval/recovery UI, explicit locked state, epoch distribution, revoke/reset. Reuse owner timelines without inbox subscriptions or agents. | With C if proven, otherwise a separate release with honest installation-authorized history wording. Takeover alone never restores old keys. |
| E. Simultaneous live devices | Per-device receiver registry/inboxes/receipts and execution authority. | Per-device transport wraps, subscriptions and primary executor policy. | Future product choice; not required for history and excluded from 1.16.1. |

If A's backend prerequisite is not ready, 1.16.1 can ship this reviewed design and accurate local wording, but cannot claim globally immediate sign-out refusal or authoritative named receiver state. A local enrolled boolean or key creation time cannot identify the receiving machine.

Later multi-device delivery keeps the archive model. Add bounded per-device transport wraps/inbox items, enrollment/revocation, independent cursors/receipts, offline-device retention and aggregate delivery semantics. Choose explicitly whether secondary delivery is display-only or whether multiple agents execute. Recommend multiple readers with one primary executor. Multi-executor support requires backend execution claims/fencing and interrupted-ownership policy. Removing `replace_others` or letting machines share today's inbox is insufficient: keys differ, first ack deletes work, and concurrent gateways can execute twice.

## Implementation review oracles

This lane runs no checks: deliverable is design, validated by owner/Orchestrator review. Future tasks name exact commands after their test owners/selectors exist; this document invents no selectors and authorizes no builds.

- Each party sees the same accepted identity in both-direction history after inbox deletion, reopen and restart. Interrupted send never creates only one owner's row. Acceptance uncertainty reconciles by id without automatic replay.
- Actual rules deny foreign UID, signed-out installation with unexpired token, revoked device, deleted generation and unbounded query. Existing Listen stops on revocation; remaining authorized devices still read.
- Racing takeovers have one revision winner. Old receiver cannot send/ack/reclaim on restart. Rotation preserves archive readability; pending old-key traffic terminates without replay. Secondary logout/delayed cleanup cannot disable new receiver.
- Online receiver logout makes friend's next send refuse, closes streams and blocks local delivery. Offline logout proves local block and explicit unconfirmed server state; network-restored cleanup proves later server fence.
- Opening, scrolling, reconnecting and bursts over 50 preserve rows without duplicates; equal timestamps use id ordering. No history read invokes gateways or advances delivery cursors. Late status cannot undo deletion.
- Archive rotation reads old retained epochs. New device without approval/recovery is locked; approved/recovered device reads without becoming receiver. Revocation stops future fetches; no test claims erasure of already copied information.
- Quota/retention boundaries refuse atomically, expire/hide as specified and release capacity idempotently. Record actual read counts, packet sizes and bounded memory; an intermediate log is not proof of the end state.
