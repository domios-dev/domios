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
are CPython 3.9–3.14 on Linux/macOS; native acceptance is pending the actual local
oracles. Other interpreters/platforms fail preflight without network admission.

Windows source admission is restricted to standard native CPython3.14.6 x64,
local fixed NTFS volumes, understood ordinary allow/deny DACL entries, and the
current process-token SID plus SYSTEM and Builtin Administrators. This trusts the
same user and these administration principals; it does not protect against their
concurrent modification or attest every runtime dependency. Other runtime versions,
venvs, debug/free-threaded builds, embedded layouts, reparse points, hard-linked
files, remote volumes, unknown ACEs and unreadable/null DACLs refuse.

Parent and worker independently open source/interpreter/runtime roots and private
cwd objects with READ_CONTROL, deny write/delete sharing, inspect volume/file IDs,
owner/DACLs and ancestor replacement rights, and revalidate captured identities.
Private cwd creation supplies a protected DACL in the creation call. The parent
retains these guards through exact exit/capture settlement; the worker compiles
and executes the contract from the exact verified bytes. No new header field,
identity fabrication, interpreter copy, ACL repair or owner-environment mutation
is involved. Path-based execution/removal does not establish an atomic guarantee.
Lost identity prevents removal, and native memory/token/file/process handle close
failures stay unconfirmed. Unknown directory creation is retained as unknown.

Windows uses only CPython's originally captured `_handle`, an exact blocking
handle wait, unsigned GetExitCodeProcess result, one TerminateProcess request and
one handle Close. Pinned subprocess source SHA256 is
`6628ffdd65c093a6c08cae01ffe82877d3ced515aac9e7be0cff16512c30a7d9`.
A different installed source byte hash refuses; no newline normalization or
runtime installation is attempted. The assumed handle layout comes from
[CPython3.14.6 subprocess](https://github.com/python/cpython/blob/v3.14.6/Lib/subprocess.py).
Opened-object security and creation follow
[GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)
and [file security](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights).
There is no arbitrary PID open, POSIX signal normalization or post-reap signalling.
Windows native qualification and an existing exact installed interpreter path
remain absent. Source admission support and injected controls are not native proof.

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
and an oversized negative. Canonical
`9eea8cef890c9582a834d3fa3ba05c5155d58395` passed both independent one-case greens and all 79 pure cases with no skips. The grouped harness repair
adds ten pure controls (two tests-first regressions and eight boundary controls).
The absolute-deadline correction adds ten more pure controls, bringing discovery
to99 authored cases, including four untouched held-out metric tests.
Expected pass99/fail0/skip0 remains deferred for this descendant. These mechanism
inputs do not qualify live data.

The first macOS native success attempt at that canonical SHA failed before
helper admission with `trusted_file_permissions`: the default framework
interpreter was root-owned mode0775. Its outer `fixture_wrong_predicate` error
lost fixture cleanup facts and listener teardown consumed approximately55s.
Retain `/tmp/codeterm-qa/fresnel-local-success-20261002-213741-27499/full.log`;
that failure establishes no helper/TLS/wire success. Do not retry unchanged.

The harness now runs production preparation, including interpreter/helper/contract
identity and private-directory admission, before creating certificate files or a
fixture listener. The prepared exact interpreter/header/deadlines are retained;
all three identity hashes are revalidated at helper preparation. The original
request and run starts precede preparation and are not reset. Failed JSON keeps
`error`/`outcome` as the primary typed failure, `oracle_error` as the independent
oracle mismatch, and full `transport`, `fixture`, `preparation`, `cancel_thread`
and `cleanup_errors` facts. A failed teardown cannot replace the primary refusal
or turn incomplete cleanup into success.

An owned socketpair wakes the selector waiting for the listener without opening
an HTTP connection. The server acknowledges pre-accept cancellation and retires
its listener, selector and wake reader; the controller observes retirement,
joins the exact thread and closes the wake writer. For accepted connections the
TLS socket is published before the blocking handshake, so cancellation shuts
down that exact handle. Held responses use the release event. Header-cancellation
waits also observe retirement, avoiding a lost-header wait after setup failure.
Shutdown is a cancellation request; only observed retirement/join/close facts
establish completion. OS noninterruptibility can still exceed the failure bound.

The two independent harness regressions were added in tests-first ancestor
`baedb6671b6d2d1dd81d56b381830503aea83693`. Fermi confirmed both intended pure
reds after real merge into `4b4de896484a726b5050728f84d79fc9152e31e1`: selected1,
exit1 each, listener construction despite preparation refusal and replacement of
the primary typed refusal. No import/setup/native failure; no repeat reds needed.
After the repair merge, the changed focused proof is:

```bash
bash -c 'cd evals/tab-state-jev && python3 -m unittest test_jev_transport.HarnessRegressionTests.test_preflight_refusal_never_opens_listener'
bash -c 'cd evals/tab-state-jev && python3 -m unittest test_jev_transport.HarnessRegressionTests.test_failed_receipt_keeps_primary_and_fixture_cleanup'
bash -c 'cd evals/tab-state-jev && python3 -m unittest test_jev_transport.HarnessBoundaryTests'
```

Expected selected/pass counts are1,1,8, with zero failures/skips. The controls
inject selector readiness, sockets, TLS streams, threads, clocks and file
operations; they execute no native IO. They cover pre-accept wake acknowledgement,
handshake and header cancellation, successful exact request/complete response,
pinned interpreter/original deadlines, changed identity refusal, failed receipt
cleanup propagation and certificate setup failure before listener admission.

Interpreter selection is the interpreter that starts the harness; there is no
new CLI/env override. On this macOS host the existing canonical candidate resolves
to `/opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14`,
current-user-owned mode0755. Read-only metadata supports selection, not executable
qualification. Use that exact path for future macOS commands so helper preparation
pins the same binary. No chmod, interpreter install/copy or trust weakening is
part of this repair. Those Linux and Windows native gaps were open at that historical repair checkpoint.
The platform continuation below records the later Linux receipt and current Windows scope.

A relative remaining-time timeout sampled by the caller can be converted using
a later admission clock and advance the original case end. `JevTransport.send`
now accepts optional `request_end`, an absolute monotonic failure bound in the
same clock domain as its injected clock. Effective hard end is exactly
`min(admission_start + timeout, original_run_end, request_end)` when supplied;
without it the existing60s/request600s/run policy is unchanged. Bounds reject
bool/nonfinite/invalid inputs and expired or consumed operation windows before
plan/lease/process admission. Typed admission failure receipts retain the original
finite absolute bound without admitting a helper. Cleanup remains5s inside the
effective bound; an earlier run/timeout limit may shorten it, never extend it.

Both harness send call sites pass captured `hard_end` directly as `request_end`
with the existing60s maximum timeout. The prepared helper header, case run end,
listener/connection, lease, capture and teardown keep the original exact ends.
The plan factory requires exact operation/hard-end equality; even a sub-microsecond
later header is refused. There is no positive tolerance or deadline regeneration.

Tests-first ancestor `220aa53739f8a1370a4e90b0699cd86b49a30500` added an independently
runnable existing-run_case plumbing control. Fermi confirmed its intended
`None !=60` assertion red on recorded send kwargs at canonical
`339fd582b86696d826d7efacc5eee655c7b359e8`, preserving `e308e714` ancestry. No new
API/import/native failure served as the red. At that same canonical SHA the
previous preparation/receipt regressions passed1 each and boundary class passed8;
those ten pure controls establish no native resource proof. No repeat reds needed.

After merging the absolute-end repair, the changed focused proof is:

```bash
TEST_CAP_SECS=600 scripts/test-capped.sh --exec bash -c 'cd evals/tab-state-jev && python3 -m unittest test_jev_transport.AbsoluteDeadlineHandoffTests'
```

Expected selected10/pass10/fail0/skip0. The class preserves the one confirmed
handoff regression and adds a deterministic2s-to12s caller/admission clock advance
through real send, exact prepared-plan/identity checks and an injected lease,
including a valid answer. Other controls prove expired bound/consumed reserve
zero-admission refusal, earlier run/timeout clamps without header rewriting,
invalid bounds, exact sub-microsecond mismatch refusal, unchanged default policy,
full expired-harness failure/fixture receipts and poisoned second-call refusal
with the same absolute end. Mocks and synthesized TLS/lease receipts exercise
mechanisms only; they are not native cleanup or TLS qualification.

The exact proposed macOS commands below are deferred, not authorization. Fermi
must review focused greens and separately authorize one actual success case first;
stop on first failure and review the full JSON/timing before any of the remaining
five cases. No retry is implied. Run each separately:

```bash
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case success
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case response_oversize
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case redirect_refused
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case held_response_deadline
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case cancel_after_headers
TEST_CAP_SECS=600 scripts/test-capped.sh --exec /opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14 evals/tab-state-jev/local_jev_transport_io.py --case partial_capture_failure
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


## Platform continuation and deferred proof

Fermi qualified the six macOS synthetic HTTPS/resource cases at canonical
`1d142119ff8c3efe3731e566e62d185a913c3fdc` using the exact Homebrew3.14.6 candidate
above. The original failed55.375664s attempt stays immutable. Focused Linux
[CI37053282843](https://github.com/rollacode/codeterm/actions/runs/37053282843)
qualified 99 pure cases (zero failed/skipped) and six synthetic native cases only
at `8def2537bfdc45c61a84f5040d496e010a4cfac1`, on actual ubunta-linux-build/x86_64,
with `/usr/bin/python3` resolving to `/usr/bin/python3.12`, CPython3.12.3 and
interpreter SHA256 `e50d468e8b0adfb05733f5b87b3cff34829c4a8c1aea50c865aa8bdfe4bb150f`.
These are Fermi-reported scoped receipts; this Worker ran no gates. They do not
qualify later source, Windows, remote inference, runtime parity, authentic data,
automatic actions, the90 live matrix or release.

Windows tests-first ancestor `31fb3cd28761af22ee56fad5f18bf912ae680033` was real-merged
at `a126424c5e4fb5da1ddc86257886a8f368bc5fc6`. Fermi confirmed selected1/exit1 for
captured Handle.Close called2 rather than1 through the existing wait/close/cancel
API. No setup/import/native error served as the red; no repeat red is requested.
The source repair preserves that assertion and adds injected ACL/identity,
protected creation, verified bytes, ABI, cleanup and receipt controls. Existing
POSIX authority controls inject status conversion and signal operations on Windows;
none is skipped. Authored counts at this platform repair: focused33 and full132
(run14, contract21, transport58, regressions35, heldout4). All are deferred.

After an ancestry-preserving merge, canonical focused commands are:

```bash
TEST_CAP_SECS=600 scripts/test-capped.sh --exec bash -c 'cd evals/tab-state-jev && python3 -m unittest test_regressions.WindowsPlatformTests.test_captured_windows_process_handle_is_closed_exactly_once'
TEST_CAP_SECS=600 scripts/test-capped.sh --exec bash -c 'cd evals/tab-state-jev && python3 -m unittest test_regressions.WindowsPlatformTests test_regressions.WindowsTrustTests test_regressions.WindowsHandleFailureTests test_regressions.WindowsNativeInspectionTests test_regressions.WindowsHarnessTests test_regressions.WindowsCompletionTests'
TEST_CAP_SECS=600 scripts/test-capped.sh --exec python3 -m unittest discover -s evals/tab-state-jev -p 'test_*.py'
```

Expected1,33,132 executed/pass respectively, zero failed/skipped; stop on the first
unexpected failure. These are injected mechanisms, not Windows resource proof.
The one consolidated suite at this repair head replaces repeated historical suites.
The shared process authority/settlement edits justify one macOS success smoke at
the same exact Homebrew interpreter after canonical review; no six-case repeat.

The separately reviewed focused workflow inputs are `evaluator_scope` (`full`,
`jev-linux`, `jev-windows`), `expected_source_sha` and `interpreter_path`. Focused
source SHA must equal dispatch workflow SHA and checkout HEAD. Existing full
jobs/triggers remain independent. There is no interpreter setup/install/fallback,
new secret or retry. Windows dispatch remains held until a real existing exact
native3.14.6 path and this complete source unit have been reviewed. No path is
invented here. `$jevQaPython` below means that separately selected existing absolute
path supplied as data, not an installation instruction. Canonical PowerShell
commands, each foreground and separately approved, are:

```powershell
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case preflight
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case trusted_directory
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case foreign_write
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case null_dacl
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case inherited_write
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --trust-case reparse_refused
& $jevQaPython -m unittest discover -s evals/tab-state-jev -p 'test_*.py'
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case success
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case response_oversize
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case redirect_refused
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case held_response_deadline
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case cancel_after_headers
& $jevQaPython evals/tab-state-jev/local_jev_transport_io.py --case partial_capture_failure
```

Read every full JSON plus immediate `$LASTEXITCODE` and Stopwatch elapsed value;
expected native wrapper exit0 means the named oracle held, not all inference
succeeded. Trust controls expect no helper/listener/network and predicates
accepted, accepted, windows_foreign_authority, windows_dacl,
windows_foreign_authority, windows_reparse_or_links respectively. Negative DACLs
and links are created only in newly owned disposable resources under a retained
protected root; inherited_write checks an actual inherited ACE. Native preflight
records token SID, NTFS/file IDs, source hashes and owner/DACL hashes, token/heap
and file handle closure and cwd removal. Unavailable runtime/ACL/symlink privileges
return `oracle=not_run`, exit1, with cleanup facts; they are never green skips.

All native controls retain original55s operation/60s hard/600s run ends and cleanup
inside5s. The six TLS oracles remain unchanged; Windows actual termination/exit
codes are reported unsigned and are not POSIX -9. Partial capture must remain
completion_confirmed=false and poison the next lease despite closed actual resources.
The focused workflow already has bounded foreground native steps; extra trust
controls above need Fermi's concrete canonical placement before execution. Outer
Actions timeout is failure containment only. No Worker gate or Windows dispatch
is authorized by this document.

Windows rollback is the six-file platform unit back to canonical `a126424c` (or
restore honest unsupported Windows admission together with its tests/docs). The
focused workflow checkpoint is independently retained; criteria/datasets and all
product/shared/protected paths require no restoration. Existing live/release holds
and separately owned platform/runtime prerequisites remain open.
