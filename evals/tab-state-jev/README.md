# Tab-state Jev evaluator source preparation

This evaluator loads the immutable shipped `criteria.json` directly. It measures
classification math over supplied human-labelled records and performs a bounded
credential-bearing HTTPS request through its own interpreter helper. Source
acceptance does not establish runtime parity or authorize inference, captures,
automatic actions, the live matrix or release acceptance. Fermi owns canonical
execution and acceptance after a real ancestry-preserving merge.

`screens.jsonl` retains the 82 historical frames. Their context predates the
current event questions. A `dev-capture` source string or a JSON provenance claim
is not proof of authenticity. This slice generates no event, identity, timing,
label, `other` frame or provider-safe option. The capture/fixture/held-out helpers
and all original criteria, datasets, reports and qualification bytes stay intact.
Future event capture and runtime parity remain separately reviewed prerequisites
on the parent's existing ownership.

## Contract and report

`request_contract.py` validates a plain label map, finite JSON values, bounded
inputs and every answer against the actual declared question map. Canonical
criteria bytes are pinned to SHA-256
`783e640d7d3e9f7f58add248cc42fb66a4a928de2fd778d9706e0c4926946f8b`.
The manifest keeps immutable criteria/dataset/question bytes. Reports record an
explicit operator-supplied source SHA (unverified origin), the reviewed source
basis, original byte hashes, explicit endpoint/model alias and question map.
Per-request Python wire bytes, canonical Python state JSON and canonical Python
question JSON have separately named SHA-256 domains. These are not Rust
serialized typed-request hashes. `jev-latest` does not identify an immutable
server model.

The screen arm declares `q`; the context arm declares `label` and
`prompt_visible`. Additional questions are carried only from a supplied immutable
`--question-manifest` JSON map. The full supplied map must include the arm's
classification questions, and its classification criteria must equal the pinned
canonical map. This evaluator does not establish that manifest's runtime origin.
All declared answers must be valid, including supplied extra questions. Supplied
context is preserved without filling missing fields or assigning identities.

Every report explicitly retains:

- `runtime_parity: unproven`;
- historical, unknown/current-claim or synthetic mechanism provenance status;
- `current_runtime_qualification.eligible: false`;
- `automatic_actions_enabled: false`.

The existing thresholds and math remain: confidence >=0.80 is strong, >=0.60
and <0.80 is weak; strong actionable precision requires >=0.90 and weak precision
>=0.60. Original held-out mathematical fields and thresholds remain under
`recovery_qualification`; they do not override top-level runtime ineligibility.
Coverage, complete-dataset selection and all answered questions fail closed.
`--allow-incomplete` is removed. Nonzero offsets and partial limits are refused
before inference. A complete selected dataset must fit the 300-request ceiling.
The evaluator stops after its first failure and never retries.

Historical variants still omit `other`. Read them explicitly with
`--criteria-mode historical-diagnostic --criteria-variant current` or
`auth-first-run`. This mode reports exact variant byte hashes and missing labels,
loads no token and makes zero requests. It neither adds labels nor qualifies old
A/B records as current data. The canonical mode refuses a historical variant flag.

For separately authorized future evaluation, the CLI requires `--source-sha` and
accepts `--arm`, `--set`, `--dataset`, `--criteria-file`, `--question-manifest`,
`--model` and the exact named-instance `--token-file`. The only admitted endpoint
is `https://api.typesafe.ai/v1/systemone`. `--timeout` must be finite, >5 and <=60
seconds; default 60. The run ceiling is 600 seconds. No ambient endpoint/model
override, implicit token-profile selection, partial promotion or retry exists.

## Credential and process authority

`jev_transport.py` validates request bytes, helper/contract/interpreter file
identity and hashes, a private empty working directory and the fixed endpoint.
It owns one sequential helper lease. Default argv is the resolved interpreter,
`-I -S -B`, and the reviewed `jev_request_worker.py`; there are no executable
hooks or subprocesses in that worker. Its descendants contract is limited to this
trusted helper source spawning none.

Only the exact required token is accepted from `JEV_API_TOKEN`,
`TYPESAFE_JEV_TOKEN` or the explicitly supplied named-instance token file. The
original credential remains in place. The token passes through the anonymous
owned stdin pipe as one length-framed binary channel, alongside a bounded parent
schema and request bytes. It never enters argv, child environment, request/result
files or diagnostics. The worker verifies source/interpreter hashes, request hash
and the original operation deadline before network admission. The parent closes
the writer and the helper closes the reader; both closures are observed.
Credentials are not guaranteed to be zeroized in Python/OS memory.

The child environment is minimal, with no proxy, Python startup/site/user hooks,
daemon credentials, provider secrets or TLS key-log/trust overrides. The worker
uses direct HTTPS sockets, certificate and hostname verification and actual
public trust roots. It records root provenance and a framed, sorted public DER
root-set SHA-256. On macOS an empty default root store falls back to the
root-owned, non-writable public `/etc/ssl/cert.pem`. No trust store is changed.
Redirects and content encoding are refused; an ambient proxy cannot be inherited.

Bounds are request 256KiB, response 64KiB, aggregate header/framing bytes 16KiB,
100 header fields, failed HTTP body 4KiB, stdout/stderr 4KiB each and read/write
chunks <=4096 bytes. Short reads, chunked bodies, headers and trailers all use
the original deadline. Excess is rejected before decoding/parsing, never
truncated into a valid answer. Failure output has typed status/size/deadline
facts and no response body, exception text or authorization header. Oversized
normalized answers also fail the bounded helper receipt rather than overflowing
stdout.

Request deadlines begin before preparation, creation or credential handoff.
Operations stop at `min(request_end, run_end)-5`; the five-second cleanup reserve
is inside both ceilings. Creation, exact exit, capture completion and cancellation
are observed through events/conditions. There are no sleeps, polling intervals,
retries, process-group cleanup or signal-after-reap paths. EOF, helper JSON and a
parent status alone cannot settle success: the exact child's exit/reap, both
bounded captures, input channel, joined ownership threads, process authority and
private working directory must all be confirmed closed.

Linux uses blocking `waitid(P_PID, WEXITED|WNOWAIT)` to observe the exact child
without reaping, then serializes cancellation with the sole `waitpid(pid,0)`
reaper. The unreaped child preserves PID authority until that lock releases it.
Darwin uses its libc `waitid` ABI, with `P_PID=1`, `WEXITED=4`, `WNOWAIT=0x20`
verified against the dispatch host's SDK `sys/wait.h`; a conservatively sized
opaque siginfo buffer is not decoded. Both set CPython Popen's returncode after
sole reaping, avoiding destructor ownership transfer. Supported source semantics
are CPython 3.11–3.14 on Linux/macOS; native acceptance is pending the actual local
oracles. Other interpreters/platforms fail preflight without network admission.

A Windows candidate adapter waits on and terminates through the originally
captured CPython `_handle`, then closes that handle; pure injected tests exercise
it. **Windows transport admission currently fails
`private_directory_unsupported`** because private-directory ACL confinement is
not implemented/proven. Windows native transport is an explicit open gap, never
a green skip or a claim inferred from the adapter tests. A Windows extension needs
Fermi's concrete disposition before any extra ownership is requested.

Kernel creation, IO, termination and filesystem close can be noninterruptible.
The failure deadline bounds waiting/admission, not a universal OS stop guarantee.
A late-created helper is retained and cancelled when its actual authority arrives;
expired/cancelled creation cannot admit credentials/network. Unconfirmed exit,
capture, channel/handle close or lost reaping authority produces a retained
`cleanup_unconfirmed` receipt and blocks further requests. Ownership threads are
non-daemon and retained: a noninterruptible OS call can delay controller exit.
An outer runner timeout is failure containment, not proof of cleanup or all
descendant termination.

## Deferred canonical gates

No gate, syntax check, import check or local IO case runs in the source lane.
Fermi has already confirmed the two intended independent reds after merging the
tests-only ancestor `7b018774ed316063bdee231554a36d0ba82e7563` into canonical as
`d30f2191404e6c30e57ad8c458e6a3d56694fd22`. Each selected one unittest case:
plain map rejected by legacy `load_criteria`, and unbounded legacy `read()` refused
by the injected response. That wrapper's exit0 means expected reds reproduced.

After merging the repair descendant, run each isolated green first:

```bash
bash -c 'cd evals/tab-state-jev && python3 -m unittest test_regressions.LegacyRegressionTests.test_canonical_criteria_plain_map_is_loadable'
bash -c 'cd evals/tab-state-jev && python3 -m unittest test_regressions.LegacyRegressionTests.test_response_reads_are_explicitly_byte_bounded'
python3 -m unittest discover -s evals/tab-state-jev -p 'test_*.py'
```

Each isolated command must select1/pass1/skip0. The response oracle uses the real
worker bound-owning reader through injection, with a valid small-response positive
and an oversized negative. Pure discovery currently authors79 cases, including
four untouched held-out metric tests; expected pass79/fail0/skip0, pending
canonical execution. These mechanism inputs do not qualify live data.

Run each local case independently before aggregation, under Fermi's separately
reviewed platform/gate disposition:

```bash
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case success
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case response_oversize
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case redirect_refused
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case held_response_deadline
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case cancel_after_headers
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 evals/tab-state-jev/local_jev_transport_io.py --case partial_capture_failure
```

Each case expects one exact wire request and one response header, with an actual
production helper, a captured loopback listener, public fixture-local EC CA/key
and synthetic token. The harness generates its public fixture certificate in
process, without a certificate-generation subprocess. The fixed listener/CA
policy is constructed only by the harness's private in-process bootstrap; it is
not a production environment/CLI allowlist. Certificate/hostname verification
stays on. Readiness, held response and cancellation use explicit event latches
that are absent from the production helper.

Success requires normalized positive answers and exact native exit/reap,
bounded capture completion, joined threads and closed owned handles/channels.
The other five cases require `response_oversize`, `redirect_refused`, `deadline`,
`cancelled`, or `cleanup_unconfirmed` respectively, with native exit and all actual
handles closed. Partial capture must retain a positive partial byte count, keep
completion unconfirmed and refuse a next request before creation/network. Fixture
listener/peer/thread receipts and one-request counts are mandatory. Unsupported
or unconfirmed semantics exit1 with failure receipts; they never skip as PASS.
No real Jev token, named dev runtime, capture, archive/version executable or
provider/model is accessed by these gates.

Rollback removes the contract/transport/helper/tests/docs evaluator unit together,
restoring the prior evaluator implementation. Original criteria, variants,
datasets, qualification and prior evidence require no restoration. Product and
protected source behavior is unchanged.
