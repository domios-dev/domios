# CTMC1 Mobile Core Wire V1

Status: authored native-core/iOS compatibility contract, replacing the proposed text in Spec `sp_b9ta7i/sec_3z5l9j`. The preserved reviewed merge is `9f8e896c8ffc53ec7b0c0644c51afcc7645eccf7` on the integrate/integration 1.10.23 lineage. Executable vectors live in `src-tauri/tests/fixtures/telegram269-mobile-core-wire-v1.json`, `src-tauri/src/telegram269_wire_contract.rs`, and the standalone macOS Swift project at `tools/MobileWireVectors/`.

This contract does not authorize dependent routes or iOS source until review. It excludes release, APNs, default/plugin packaging, production, and App Store work. It does not change PTY delivery, submit recovery, delivery evidence, the delivery ledger, or protected hooks. Mobile routes never accept or return a daemon bearer token.

## Records, proof classes, and grants

`MobileClientRecordV1` contains exactly `client_id`, `display_name`, `client_public_key`, `client_key_fingerprint`, `server_instance_id`, `endpoint_id`, `grants`, `grant_revision`, `state`, `created_at_ms`, `updated_at_ms`, and `accessories`. State is `pending|active|revoked`. The phone generates Ed25519 keys with CryptoKit `Curve25519.Signing` and retains the private key in device-only Keychain. Core stores the approved public key and separate core-owned signing identity.

A `pairing` proof can create or inspect only its own pending pairing request. An `active` proof requires the stored active key and an unexpired grant covering the exact route and target. The only grants are `directory.read`, `conversation.read`, `theme.read`, `events.subscribe`, and `chat.send`, each with explicit targets. Prospective, unknown, expired, rejected, and revoked keys cannot call normal routes.

## Canonical JSON, base64url, and request proof

JSON bodies and signed objects are RFC 8785 JCS UTF-8. Digests cover the exact transmitted JCS bytes; duplicate keys, noncanonical bytes, NaN, and Infinity reject.

Authorization is exactly `CodeTerm-Mobile-v1 <token>`, where token is a nonempty unpadded RFC 4648 base64url encoding of the JCS proof object. The alphabet is `[A-Za-z0-9_-]`; `=`, whitespace, `+`, `/`, non-ASCII, and nonalphabet bytes reject. Length modulo four may be 0, 2, or 3; remainder 1 rejects. For a 2-character last quantum the second sextet's low four bits must be zero; for a 3-character last quantum the third sextet's low two bits must be zero. Decoders reject rather than normalize nonzero unused bits. Normative vectors include `Zg`, `Zm8`, and `Zm9v`, plus rejection of empty authorization tokens, `A`, `Zg==`, `Z*`, `Zh`, and `Zm9`.

The proof object has exactly:

```json
{"body_sha256":"sha256:<64 lowercase hex>","canonical_target":"/api/mobile/v1/...","client_id":"mc_alpha","endpoint_id":"https_main","expires_at_ms":1700000300000,"issued_at_ms":1700000000000,"key_fingerprint":"sha256:client-key","method":"POST","nonce_128":"AAECAwQFBgcICQoLDA0ODw","proof_type":"active","server_instance_id":"daemon_alpha","signature":"<base64url>"}
```

Remove `signature` before signing; unknown members reject. `nonce_128` decodes to 16 bytes. Method is uppercase. Lifetime is at most 300 seconds with at most 300 seconds clock skew. Nonces are single-use and retained at least 24 hours.

Canonical targets contain only absolute path and query. Paths have no dot segments. Decode existing escapes once, remove `.` segments and resolve `..` without allowing traversal above root, then percent-encode UTF-8 with uppercase hex except RFC 3986 unreserved bytes and path separators. Query pairs retain duplicates, sort by encoded key then value, use `%20` for spaces, and retain empty values. A decoder recomputes the canonical target and rejects noncanonical alternatives rather than accepting lowercase escapes, dot segments, unsorted pairs, malformed escapes, fragments, or relative paths.

Let `B(x)` be big-endian u32 byte length followed by bytes and `U64` unsigned big-endian. Exact request signature bytes are:

```text
ASCII("CTMC1-REQUEST\0") ||
B(proof_type) || B(client_id) || B(key_fingerprint) ||
B(server_instance_id) || B(endpoint_id) || B(method) || B(canonical_target) ||
body_sha256:32 || U64(issued_at_ms) || U64(expires_at_ms) || nonce:16
```

The signature member is base64url of the raw 64-byte Ed25519 signature over those bytes.

## Exact target authority

Target bytes begin `ASCII("CTMC1-TARGET\0")`. Kind `0x00` is `none` and has no fields. It is required for discovery/preapproval, machine/client/accessory directory records, and pending pairing status, and authorizes no conversation, subscription, send, or request status.

Kind `0x01` is `tab`, followed by:

```text
B(gateway_instance_id) || peer_present:u8 || [B(peer_id)] ||
B(target_instance_id) || B(tab_id) || B(session_id) ||
U64(lifecycle_epoch) || binding_nonce_digest:32
```

Direct targets have gateway equal to target instance and no peer. Relayed targets name gateway, peer, and final target separately. The gateway checks endpoint, grant, and full tuple; the receiver rechecks its own instance, session, lifecycle, binding digest, and grant provenance. It never remaps a mismatch to General.

There is no durable `session_generation` authority. The reviewed core declares `Tab.session_binding_nonce`, `Tab.reported_session_binding_nonce`, and `Tab.lifecycle_epoch`. The receiver rereads them, requires the two nonce values equal, and compares `binding_nonce_digest = SHA256(raw session_binding_nonce)` and lifecycle epoch. The internal orchestrator launch generation is not exposed.

## Discovery and pairing

Discovery is a typed `announce.web_ui` endpoint with exactly `endpoint_id`, `origin`, `transport`, `server_instance_id`, `server_key_id`, `server_public_key`, `server_key_fingerprint`, HTTPS `tls_spki_sha256`, `protocol_version`, `directory_revision`, `build_sha`, and detached signature.

Exact discovery identity bytes are:

```text
ASCII("CTMC1-DISCOVERY\0") || B(endpoint_id) || B(origin) || B(transport) ||
B(server_instance_id) || B(server_key_id) || B(server_key_fingerprint) ||
B(tls_spki_sha256) || B(protocol_version) || U64(directory_revision) ||
B(build_sha) || server_public_key:32
```

The advertised Ed25519 key verifies the signature but does not bootstrap trust. HTTPS uses normal certificate validation and requires the leaf SPKI digest to equal `tls_spki_sha256`. A 100.x address, HTTP, self-signed certificate, ATS downgrade, or self-signature is insufficient. Approval pins origin, server instance/key, endpoint, client fingerprint, grants, and targets; identity changes require approval again.

Pairing routes are `POST /api/mobile/v1/pairing/requests`, `GET /api/mobile/v1/pairing/requests/{id}`, local-owner approve/reject, and client revoke. Create contains exactly protocol/client key and fingerprint/display name/server/endpoint/target selection/grants/times/client signature. `target_selection` is `owner_select` or owner-supplied tab tuples. Pairing expires within ten minutes. Status returns only its state, expiry, server/endpoint, and `none`; no inventory. Approval/rejection requires existing local authentication and a live local owner tab validated by tab/session authority; prospective, mesh, and title-only callers are denied.

## Detached authenticated responses

Every authenticated body is JCS and contains no signature member. Headers are:

```text
X-CodeTerm-Mobile-Response-Key: <server key id>
X-CodeTerm-Mobile-Response-Nonce: <base64url 16 bytes>
X-CodeTerm-Mobile-Response-Signature: <base64url 64 bytes>
X-CodeTerm-Mobile-Request-Id: <request id>
X-CodeTerm-Mobile-Server-Time-Ms: <unsigned decimal>
```

Exact signature bytes are:

```text
ASCII("CTMC1-RESPONSE\0") || B(status_decimal_ascii) || B(request_method) ||
B(canonical_target) || request_nonce:16 || B(request_id) || B(client_id) ||
B(server_instance_id) || B(server_key_id) || request_body_sha256:32 ||
response_body_sha256:32 || response_nonce:16 || U64(server_time_ms)
```

Request nonce and id appear exactly once in the signed input. Request and response digests occupy distinct fixed positions. The response signature and all signature headers are excluded from the body digest and signature input, preventing a cycle. On every successful response, request id and server time are transmitted in the named headers; key id and response nonce are transmitted alongside them, while method, canonical target, request nonce, request body digest, client id, and server instance are already bound request/connection inputs. Thus a verifier receives or already possesses every signature input. Errors have exactly `error_code`, `message`, `request_id`, `details`, and `server_time_ms`, and use the same detached framing.

## Send, status, and durable suppression

The only send route is:

```text
POST /api/mobile/v1/gateways/{gateway_instance_id}/targets/{target_instance_id}/tabs/{tab_id}/sends
```

Its JCS body has exactly `target`, `run_id`, `client_request_id`, `correlation_id`, `text`, and `reply_mode`. Target bytes must equal the route and grant tuple. IDs are 1–128 ASCII bytes; text is nonempty UTF-8 at most 32 KiB; reply mode is `accepted_only|await_reply`. The receiver enforces the entire current tab target immediately before reserving.

Success is authenticated HTTP 202 `SendReceiptV1` with exactly `request_id`, `client_id`, `client_request_id`, `correlation_id`, `target`, `reservation_state`, `accepted_at_ms`, `delivery_id`, and `status_path`. Initial reservation state is `reserved|submitted`; delivery and status path are string or null according to existing evidence.

Before any PTY operation, core durably reserves `(client_id, client_request_id)` with canonical body and target digests. Same id/digest concurrent calls return the same receipt and submit at most once. Different digest is `409 client_request_reuse`. States are `reserved|submitted|confirmed|unknown|failed|expired`. Crash or lost evidence turns reserved/submitted into terminal `unknown`; core and clients never auto-resend and responses contain no resend instruction.

`GET /api/mobile/v1/requests/{client_request_id}` requires the owning active client and current `chat.send` for the exact target. Another client gets `404 request_not_found`; removed grant `403 grant_denied`; revoked client `401 client_revoked`. It returns stored receipt plus `state`, `evidence`, and `updated_at_ms`.

At 24 hours, the required implementation turns the record into a durable `expired` tombstone retaining client/request ids, body digest, target digest, and state. It survives restart and normal cleanup and is never reused. Same digest returns `409 reservation_expired`; different digest returns `409 client_request_reuse`. The `ReservationStore` clone/`HashMap` exercised by the COD-1 Rust vectors is explicitly a model of these transitions only; it is not evidence of durable storage, restart recovery, or concurrency behavior. Durable storage and real route enforcement are COD-2/COD-3 implementation acceptance. This wrapper does not alter PTY write, recovery, evidence, or ledger behavior and makes no exactly-once claim about them.

## Subscriptions and SSE chain

`POST /api/mobile/v1/events/subscriptions` carries the target set in its JCS body, not the request line. Body members are exactly `targets` (1–64 complete tuples), `requested_lifetime_s`, and nullable `client_cursor`. `target_set_sha256 = SHA256(JCS(targets))`. Response is exactly `subscription_id`, `target_set_sha256`, `boot_id`, `sequence`, `directory_revision`, `grant_revision`, and `expires_at_ms`.

The compact stream target is `GET /api/mobile/v1/events/stream/{subscription_id}?cursor={boot_id}.{sequence}`. There is no targets query. Boot id is 1–64 `[A-Za-z0-9_-]`; sequence is unsigned decimal without leading zero except `0`; cursor is at most 128 bytes. This explicit subscription encoding keeps the request line within 2048 bytes. Other queries are limited to 4096 bytes. A fresh proof binds endpoint, client, subscription, target-set digest, grant revision, and cursor. Wrong or evicted boot/predecessor returns `409 cursor_reset`.

The stream ends at the earliest proof, grant, subscription, or 30-minute expiry. It emits `auth.expiring` when possible, then closes. Grant narrowing closes affected streams; revoke closes all. Reconnect requires fresh proof and rechecks owner, endpoint, grants, full targets at receivers, target-set digest, boot, cursor, and stored predecessor hash. A new subscription starts a new chain.

Each SSE event has one `data:` value containing a JCS envelope with exactly `key_id`, `response_nonce`, `previous_frame_hash`, `signature`, and `signed_payload`. SSE cannot attach new HTTP headers per event. `signed_payload` is a separate JSON object with exactly `protocol_version`, `boot_id`, `sequence`, `directory_revision`, `event_type`, `target`, `run_id`, `correlation_id`, `delivery`, and `payload`; nullable values are explicit null. It contains no signature, nonce, key id, or previous hash. The envelope `previous_frame_hash` is lowercase `sha256:<64 hex>`, nonce and signature are canonical base64url, and `key_id` selects the pinned server key. Event types are `message.created`, `message.updated`, `message.completed`, `theme.changed`, `directory.changed`, `accessory.changed`, `grant.changed`, `client.revoked`, and `auth.expiring`.

```text
seed_input = ASCII("CTMC1-SSE-SEED\0") || B(subscription_id) || B(boot_id) ||
  B(client_id) || B(server_instance_id) || B(endpoint_id) ||
  target_set_sha256:32 || U64(grant_revision)
chain_seed = SHA256(seed_input)

frame = ASCII("CTMC1-SSE-FRAME\0") || U64(signed_payload.sequence) || previous_frame_hash:32 ||
  B(subscription_id) || B(client_id) || B(server_instance_id) || B(endpoint_id) ||
  B(JCS(signed_payload))

signature_input = ASCII("CTMC1-SSE-SIGNATURE\0") || frame || response_nonce:16
```

The first previous hash is the seed; later previous hashes are `SHA256(complete prior unsigned frame)`. The signature is Ed25519 over `signature_input`; the entire envelope, including signature, is excluded from `frame`. Reconnect presents the last verified `boot_id.sequence`, and the server binds the new proof to the stored hash of that exact frame before emitting its successor. Clients verify envelope key/nonce/hash syntax, subscription/client/endpoint, boot/sequence, predecessor, canonical signed-payload bytes, and signature before exposing the event.

## Exact route bodies and ownership

The shared corpus `contract.routes` is the machine-readable registry for pairing create/status/approve/reject, client revoke, grant replacement, directory read, conversation read, theme read, and request status. Every entry fixes method, path, and emitted JCS body bytes. Implementations canonicalize those bodies and compare the emitted UTF-8 bytes, not parsed object equality. The pairing status body exposes `none`, never inventory. Approval and rejection are local-owner operations. Grant replacement uses `expected_grant_revision`; removing a grant takes effect immediately and closes affected streams.

The five and only five grant names are `directory.read`, `conversation.read`, `theme.read`, `events.subscribe`, and `chat.send`. Request-status ownership is opaque: a foreign client receives `request_not_found`, the owner after `chat.send` removal receives `grant_denied`, and a revoked owner receives `client_revoked`. Receiver authorization compares `tab_id` and `session_id` as distinct fields and rejects a mismatch in either. An `unknown` reservation is terminal and never suggests, queues, or performs automatic resend.

## Source-normalized safe surfaces

`MessageV1` is exactly `id`, `type`, `content`, `timestamp`, `seq`, `collapsed`, `tool_call`, `tool_result`, `system_subtype`, `timeline_marker`, and `unsupported_families`. Type is `text|thinking|tool_call|tool_result|system`; content is at most 64 KiB. Unsupported families are a sorted unique subset of `attachments`, `tool_input`, `tool_rich_result`, `file_diffs`, `interactive_elements`, and `provider_specific_block` and remain visibly named rather than silently reinterpreted.

Non-null `tool_call` is exactly `id`, `name`, `summary`, `detail`, `error`, with 128-byte ASCII id, 128-scalar name, 4 KiB summary, 16 KiB detail, and nullable error. Raw arbitrary input is excluded. Non-null `tool_result` is exactly `tool_id`, `content`, `error`, `exit_code`, `duration_ms`, `interrupted`, with 64 KiB content. These are the bounded source-normalized subset of `codeterm_types::agent::ChatMsg`. Unsupported source families return `unsupported_message_family`; raw or generic fallback is forbidden.

A conversation page is exactly `target`, `directory_revision`, up to 100 messages, and `next_cursor` (opaque ASCII up to 128 bytes or null).

A theme response is exactly `protocol_version`, target (`none|tab`), `theme_revision`, and the current `ThemeConfig` members: `name`, `background`, `foreground`, `cursor`, `selection`, eight normal colors, and eight bright colors. Name is at most 128 scalars; every color is at most 128 scalars and matches `#[0-9A-Fa-f]{6}`. Unsupported formats return visible `unsupported_theme`, never fallback, paths, credentials, or source payload.

Directory uses opaque cursor and `limit=1..64`; 64 is the only directory page maximum. Response is exactly `directory_revision`, `items`, and `next_cursor`. Machine items contain safe machine metadata; client items contain client lifecycle, grants/revision, and accessories. Non-conversation records use `none` and never grant conversation/send authority.

## Phone-derived Watch

`PUT /api/mobile/v1/clients/{client_id}/accessories/{accessory_id}` requires an active proof from that same phone and `If-Match` equal to the current local directory revision. JCS body is exactly `accessory_id`, `kind:"watch"`, `parent_client_id`, `display_name`, `state` (`connected|disconnected|unknown`), bounded accessory UI `capabilities`, and `updated_at_ms`. Core forces parent to the authenticated client, rejects key/grant/machine-capability claims, increments directory revision, and emits `accessory.changed`. Watch has no independent key, grant, proof, route, or mesh authority.

## Limits, errors, and executable evidence

Limits: request line 2048 bytes; stream cursor 128; other query 4096; ordinary JCS body 64 KiB; pairing body 16 KiB; send body 32 KiB; IDs 128 ASCII; labels 128 scalars; grants 32; subscription targets 64; directory page 64; conversation page 100; SSE event 64 KiB; pairing expiry 10 minutes; proof lifetime/skew 300 seconds; nonce/reservation/tombstone retention 24 hours or longer. Oversize input fails before lookup.

Stable errors include `invalid_signature`, `unknown_proof_field`, `unsupported_version`, `body_digest_mismatch`, `replayed_nonce`, `stale_timestamp`, `proof_expired`, `server_binding_mismatch`, `endpoint_binding_mismatch`, `prospective_key_not_allowed`, `pairing_expired`, `pairing_rejected`, `owner_guard_denied`, `grant_denied`, `grant_revision_conflict`, `client_revoked`, `target_not_found`, `target_session_mismatch`, `target_lifecycle_mismatch`, `target_binding_mismatch`, `request_not_found`, `client_request_reuse`, `reservation_unknown`, `reservation_expired`, `cursor_reset`, `body_too_large`, `unsupported_theme`, `unsupported_message_family`, and `rate_limited`.

The preserved canonical Rust QA result for merge `9f8e896c8ffc53ec7b0c0644c51afcc7645eccf7` is `1 passed, 0 failed` from:

```bash
./build.sh test-one app telegram269_wire_contract
```

The Rust test consumes shared fixture bytes, constructs request/discovery/target/response/SSE inputs, compares exact emitted canonical bytes and deterministic Ed25519 signatures, mutates signed bytes and chain/grant state, and executes base64url, target receiver, send/reservation/crash/tombstone model, grant removal, safe schema, page-limit, wrong-boot, and Watch-parent behavior. It also emits canonical JSON and targets for number representation, escaping, UTF-16/non-BMP key ordering, percent escaping, dot segments, and sorted encoded query pairs, and rejects duplicate keys, malformed JSON/percent escapes, and noncanonical alternatives. It does not compare labels.

The committed standalone Swift oracle consumes that same corpus directly and runs with exactly:

```bash
xcodebuild -project tools/MobileWireVectors/MobileWireVectors.xcodeproj -scheme MobileWireVectors -destination 'platform=macOS' test
```

The Swift harness compares emitted canonical bytes, binary framing, deterministic CryptoKit Ed25519 signatures, and decoder refusal behavior. It does not edit or build the iOS app checkout. It is cross-language codec/vector evidence only, not native-app proof. CryptoKit on this macOS harness belongs to the same Apple API family expected on iOS, but a macOS unit-test pass does not prove iOS deployment-target availability, Keychain policy, networking/SSE behavior, lifecycle integration, or native UI behavior.

The artifact, Rust result, and any standalone Swift result do not prove durable restart/concurrency persistence, live authorization, or enforcement in real receiving routes. Those remain dependent COD-2/COD-3 implementation acceptance and must be demonstrated against durable storage and the actual gateway/peer/receiver paths.

<!-- revision: 2 -->
