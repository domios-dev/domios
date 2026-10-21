# Shared producer/consumer vectors — TEST ONLY

The RFC 8032 key in these packets is public test material; production never trusts it.

## Files
- `grant-vectors.json`: signed grant vectors for the online policy epoch. The same bytes live in `codeterm-firebase` at `functions/test/fixtures/grant-vectors.json`; both suites pin its SHA256, so a one-sided edit fails both.
- `jcs-vectors.json`: producer JCS conformance vectors.

## Online epoch time model
- `refresh_after = issued_at + 1800`, `expires_at = issued_at + 3600`, `grace_until = expires_at`. There is no offline grace and no paid tail.
- Access ends at `min(expires_at, period_end)`: an earlier authoritative `period_end` stops paid access before the signed expiry.
- States are half-open: `not-yet-valid` before `not_before`, `expired` from the effective end, `fresh` before `refresh_after`, `stale` otherwise.
- The epoch supersedes the earlier seven-day paid lease, fourteen-day grace and admin 24 h + 72 h windows; grants built on those equations fail `check_invariants`.

## Packet sections
- `vectors`: request, caller context, full schema-2 response with detached grant, and exact payload bytes. The producer suite re-signs every non-mutated vector with `signGrant` and requires identical bytes and signature; the consumer verifies each through `verify`. `tampered-payload` and `unknown-key-id` are post-issuance mutations, never producer output.
- `windows`: time checkpoints both sides evaluate against the vector's signed payload.
- `refusals`: field overrides on a vector that the producer's `validatePayload` refuses and the consumer's `check_invariants` refuses with the named defect.

Fixed issuance `T = 1700000000`, project `demo-domios`, env `test`, epoch `fixture-online-v1`, enforcement disabled.
