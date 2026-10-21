# Desktop producer contract export — TEST ONLY

Pinned accepted source: `97f420cfcd0cb20a64f7b139fcc6c3abea09f9a7`.
No real endpoint, Auth, Firestore, key provisioning, SDK, provider transport, deployment, activation or production authority was used. The RFC8032 key is public TEST ONLY material. Production must never trust it. Delivery contains public raw/SPKI key only; the regeneration script reads the existing test seed from the pinned repository fixture into process memory, not from this packet. No private seed/key is delivered.

## Files
- `vectors.json`: nine portable request/response/context vectors, exact transmitted base64url payload/signature, exact UTF-8 string, hex bytes, SHA256 and byte count; public key identity/SPKI/raw key.
- `validation.json`: executed source-signing/JCS/signature/context assertions.
- `jcs-vectors.json`: existing source JCS conformance vectors.
- `source-manifest.json`: commit and SHA256/byte lengths of exact Git blobs used, including existing test signing/grants fixture digests. No fixture private seed content.
- `signed-entitlements.ts`, `application-access.ts`, `entitlements.ts`, `service.ts`: exact pinned source definitions/implementation for review (unchanged copied Git blobs). These copies are contract references, not a runnable deployment.
- `regenerate.cjs`: deterministic generator, requires Node22 and existing repository Functions TypeScript dependency; no install. Reads pinned blobs through git show and transpiles in memory. Real createEntitlementsService/signGrant/JCS are executed. Snapshot/revision store are explicitly synthetic in-memory injections, NOT committed authority. No Firestore/Auth/provider APIs are invoked.
- `verify-portable.cjs`: independent Node crypto verification of delivered public artifacts, no repository/private fixture required.
- `commands.json`: exact commands/numeric exits and deterministic rerun result.

## Actual wire contract
Request: `{}` or `{uid:self}` preserves unsigned schema2; optional `installation_id` plus optional `uid:self` enables a bound grant only when reviewed policy/signer/linked admission exist. Implemented parser allows ONLY uid and installation_id. Foreign uid or unknown fields refuse. Installation must be1..128 Unicode scalar characters, no controls/unpaired surrogates/slash/backslash or dot/dotdot. Caller context authenticates separately; no body token/provider/epoch/admin authority.
Response: `ApplicationEnvelope & {grant?:DetachedGrant;consent?:{state:"off"|"on",revision:number}}`.
- envelope `schema_version:2`, `billing:Effective|null`, `billing_inventory:"verified"|"requires_inventory"`, `application_access`.
- application_access `schema_version:1`, authority admin|billing|free, full_access, max_mesh_machines/max_workspaces numeric|null, priority_support, sponsor, optional admin_generation.
- billing `schema_version:1`, plan free|pro|sponsor, capabilities max_mesh_machines/max_workspaces numeric|null, sponsor/priority_support boolean, period_end numeric|null.
- grant exactly payload/signature/key_id/alg; payload is base64url EXACT JCS UTF-8, signature base64url Ed25519. Neither payload nor envelope is JWT. Algorithms/key allowlist max2 are pinned by reviewed config.
- signed payload `schema_version:1`, issuer/audience/sub/inst/revision/policy_version/policy_epoch; absolute issued_at/not_before/refresh_after/expires_at/grace_until; authority/label/enforcement; scoped limits; capabilities/perks/period_end; bounded optional transition_reason. Exact complete definitions are copied in signed-entitlements.ts.
There is NO response schema3 or scalar-v3 emitter: “v3” is design revision terminology. Legacy schema2 fields remain numeric. The service currently emits capabilities=[] and does not populate optional transition_reason; absent reason never means revoked. Optional consent is unsigned metadata. No consent revision input exists on EntitlementsRequest. Missing installation ID, signer or policy returns existing envelope without grant; missing linked admission also preserves unsigned envelope. These examples do not claim an installed callable currently emits signed authority.

## Vectors and consumer interpretation
Fixed issuance T=1700000000; test project demo-domios/env test, fixture-user/fixture-installation. All policies here have enforcement.enabled=false, rollback=false; no production PhaseB activation is implied.
- valid-paid: revision10, finite paid period T+604800; lease T+604800, grace T+1209600.
- valid-admin: exact pinned existing658-byte fixture/signature, revision1/fixture-phase-a; period_end=null, lease T+86400, grace T+259200 (72h total).
- signed-free-downgrade: revision11 vs cached paid10, scoped2(account machines)/2(installation workspaces)/1000(installation mem.items), both perks false, no capabilities, period_end=null, rollback=false, lease T+604800/grace T+1814400. It replaces verified cached paid online; it is not a rollback hint and does not disable a compiled baseline.
- expired-lease: SAME valid paid bytes evaluated at expires_at equality. Lease is expired; offline retained grace may still apply under the reviewed consumer policy. Do not equate this with invalid signature or past-grace fallback.
- past-grace: same bytes at grace_until equality; outside retained authority, compiled-free fallback.
- wrong-installation: valid producer output, but caller installation differs; reject context.
- older-policy-epoch: valid signature issued under fixture-phase-a, expected fixture-phase-b. Reject inequality; “older” is this test's scenario name, not lexicographic/numeric ordering. Expected current epoch is an explicit trusted build/policy input, never learned from an untrusted payload. Revisions compare within the accepted account/audience/installation/policy context; no invented epoch ordering or reset rule.
- tampered-payload: POST-ISSUANCE test mutation of canonical revision bytes, retaining original signature. Signature false; never a producer output or malformed payload accepted by backend.
- unknown-key-id: POST-ISSUANCE detached-envelope mutation. Mathematical signature verifies if explicitly tested with the fixture public key, but trusted key dispatch MUST reject unknown ID (no try-all-keys fallback). Original signed bytes are unchanged; not a producer-emitted key choice.

`expected` is a focused vector oracle, not a full consumer implementation. Verify trusted key/algorithm/size/exact bytes BEFORE trusting/parsing payload, then canonical/schema/bindings/scopes/time/epoch/revision. The portable verifier parses only after mathematical verification for valid-byte cases; the tampered case is not parsed. Diagnostic public-key verification of unknown ID is explicitly not trusted admission. Full parser duplicate-key/oversize/nonfinite/etc matrices remain in accepted source tests, not recreated as this nine-vector packet. The unsigned envelope is a service fixture response; later offline evaluation does not magically refresh it.

All dates are fixed Unix seconds. No cache refresh extends deadlines. refresh_after=T+86400 is consumer refresh metadata, not producer gating. Expired examples are generated at T where signGrant accepts, then evaluated later; validatePayload rejects signing them at expiry/grace equality. No backend accepts an already expired fresh grant. Admin/paid/free policy equations are in the actual validator and service.

## Commands / limitations
Run `node regenerate.cjs C:/Developer/codeterm/codeterm-firebase` in this artifact folder (or pass another checkout containing pinned commit and existing functions/node_modules). Then `node verify-portable.cjs`. No package install/build/emulator required.

Source PASS is not real signing custody/current Auth/canonical-owner/registered-install admission, production history, SDK contention or actual desktop/native E2E. Financial/F1/parity/current-reader/provider/signing/namespace/rules/IAM/config/deploy/runtime holds remain. Local artifacts only; root routes this packet to Elgar/Fable. No repository edits, commits or merges.
