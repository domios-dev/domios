# Shared producer/consumer vectors — TEST ONLY

The RFC 8032 key in these packets is public test material; production never trusts it.

## Files
- `grant-vectors.json`: signed grant vectors for the online policy epoch. The same bytes live in `codeterm-firebase` at `functions/test/fixtures/grant-vectors.json`; both suites pin its SHA256, so a one-sided edit fails both.
- `jcs-vectors.json`: producer JCS conformance vectors.
- `limits-vectors.json`: shared limit-shape cases for `teams` and `agent.tabs` (see below).

## Online epoch time model
- `refresh_after = issued_at + 3600`, `expires_at = issued_at + 86400`, `grace_until = expires_at`. There is no offline grace and no paid tail.
- Access ends at `min(expires_at, period_end)`: an earlier authoritative `period_end` stops paid access before the signed expiry.
- Desktop states are half-open: `not-yet-valid` before `not_before - 300` (saturating), `expired` from the effective end, `fresh` before `refresh_after`, `stale` otherwise. The unchanged producer checkpoints use the issuer clock; desktop assertions apply the bounded clock tolerance at the start only.
- The epoch supersedes the earlier seven-day paid lease, fourteen-day grace and admin 24 h + 72 h windows; grants built on those equations fail `check_invariants`.

## Packet sections
- `vectors`: request, caller context, full schema-2 response with detached grant, and exact payload bytes. The producer suite re-signs every non-mutated vector with `signGrant` and requires identical bytes and signature; the consumer verifies each through `verify`. `tampered-payload` and `unknown-key-id` are post-issuance mutations, never producer output.
- `windows`: time checkpoints both sides evaluate against the vector's signed payload.
- `refusals`: field overrides on a vector that the producer's `validatePayload` refuses and the consumer's `check_invariants` refuses with the named defect.

Fixed issuance `T = 1700000000`, project `demo-domios`, env `test`, epoch `fixture-online-v1`, enforcement disabled.

## Limit keys `teams` and `agent.tabs`
`limits-vectors.json` is shared byte-for-byte with the producer (`functions/test/fixtures/limits-vectors.json`); both suites pin its SHA256. It is data, not a signed packet: the desktop signs each case with an ephemeral test key, and the producer runs `validatePayload` and `catalogLimits` on it.
- Free: `teams` max 6 and `agent.tabs` max 35, both installation scope (live tabs of every kind count). Paid and admin: `null`.
- The producer signs the two keys only when `policy_version >= 2` and never adds them to the schema-2 envelope, because released desktops parse signed `limits` and `application_access` with unknown fields refused.
- The desktop accepts both shapes. An absent key takes the signed authority's catalog row: free gets the compiled baseline, paid and admin get `null`. The signed vectors in `grant-vectors.json` are the old shape and verify unchanged.

## Local rollback bounds

Stored grants require their account and installation revision floor. Missing or unreadable floors refuse stored access; unreadable floors are preserved. Authenticated replies raise observed time to signed issuance even when the grant is refused, so reaching the backend after a clock rollback can expire cached access. Captured gates also expire against wall time without waiting for a monotonic timer.

Restoring the entire local snapshot, including its floors, while blocking the backend remains an inherent residual: local state alone cannot prove that a later observation occurred.
