# Canonical gate commands

Source authored only; this attempt executed none. These are concrete deferred command tokens, not authorization. Live provider creation is blocked by isolation disposition, final Auditor correlation, genuine disagreement provokers and confined executable/update bindings. Setup leaves provider updates disabled. The focused feed mechanism already passed within Sentinel28; no repeat feed regression is requested. Its live cell remains NOT RUN.

Run start/stop in a Team-bound utility shell; QA is serialized on the physical machine. Canonical promptmatrix:7794 now contains a retained failed Claude2.1.286 assembly attempt. Do not rerun prepare, reuse/replace its sidecar or execute any native command below without a separate exact fixture disposition; the failed fixture is preserved unchanged.

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --prepare
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --start
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --smoke
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --create-owner --provider codex
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --disconnect-jev
```

Candidate package pairs are pinned in versions.json: Claude 2.1.286 -> 2.1.287, Codex 0.159.3 -> 0.160.0, OpenCode 1.18.33 -> 1.18.34. These literals replace all PM_OLDER variables. Registry existence is not executable or real-prompt proof; install_older refuses pending update_fixture binding. Cursor archive exists locally but its manifest has no updater capability, so update remains pending. Full remaining source requirements are in README.md.

## Jev off

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case update --jev off --older-version 2.1.286
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update --jev off --older-version 0.159.3
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case update --jev off --older-version 1.18.33
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case update --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev off --variant default
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev off --variant busy
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev off --variant fail
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant git --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant fake-ci --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant regex-no-jev --jev off
```

## Jev on

Connect Jev with a token supplied on stdin; do not place it in argv or a committed file.

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --connect-jev
```

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case update --jev on --older-version 2.1.286
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update --jev on --older-version 0.159.3
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case update --jev on --older-version 1.18.33
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case update --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev on --variant default
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev on --variant busy
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version 0.159.3 --jev on --variant fail
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant git --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant fake-ci --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant regex-no-jev --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant budget-exhausted --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant activity-feed --jev on
```

## Additional disagreements

These four requirements are additional to the original 86 executable cells. The internal point exists in candidates.rs::spawn; the emitted point field and real differing-arm provoker do not. Each command must exit 3 (PENDING) before provider creation until those bindings land. They are retained in report coverage. The final authoritative Auditor covered-point list may add more exact commands before live authorization.

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case permission --jev on --variant disagreement --decision-point scrape_candidate
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case permission --jev on --variant disagreement --decision-point scrape_candidate
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case permission --jev on --variant disagreement --decision-point scrape_candidate
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case permission --jev on --variant disagreement --decision-point scrape_candidate
```

## Validator, report and teardown

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-demo
python3 evals/prompt-matrix/oracle_tests.py --red-sentinel-demo
python3 evals/prompt-matrix/oracle_tests.py --red-binding-demo
python3 evals/prompt-matrix/oracle_tests.py --red-courier-demo
python3 evals/prompt-matrix/oracle_tests.py --red-sentinel-provenance-demo
python3 evals/prompt-matrix/oracle_tests.py
python3 evals/prompt-matrix/report.py --root target-check/evals/prompt-matrix/promptmatrix
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --remove-owner
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --stop
```

Expected observations / exits:

- `--red-demo`: exit 1, `Expected exactly one owner notification`.
- `--red-sentinel-demo`: exit 1, `Expected exactly one claim and completed action per transition`.
- `--red-binding-demo`: exit 1, `Duplicate or stale observation id`.
- `--red-courier-demo`: exit 1, `Restart claims lack one independent launch/version receipt per tab`.
- `--red-sentinel-provenance-demo`: exit 1, `Raw Sentinel journal invented a recipient receipt`.
- Pure oracle suite: exit0,217 expected checks (166 canonical verified plus51 owned-helper source controls). Synthetic validator controls do not bind real producer fields.
- Focused Rust feed regression: already reported PASS within Sentinel28; no repeat requested. Historical exact command: `./build.sh test-one app supporter_sentinel_verdict_is_visible_in_activity_feed`. Retain original6029471427 source-expected-red checkpoint; no executed feed red is invented.
- `--smoke`: exit 0 only on real named-instance read/help responses, matching identity and canonical build SHA. Smoke proves no provider cell or transcript receipt. Provider observer creation additionally requires actual AgentReady, own session and resolved isolation.
- Every policy cell: exit 0 only on real prompt, correlated completed observations/paired arms and independent far end. Current unbound cells exit 3 before creation. Permission retains its request and shows zero answers/tool effect. Courier variants require one real install, safe Skip and own resumed-session version; busy requires install-before-idle-before-restart; failure requires zero restarts and one actual owner notice.
- Report: exit 1 until all 90 executable requirements plus any further Auditor points pass. Only budget-off N/A is accepted. All pending contract statuses and an empty/pending covered-point list are independent nonzero blockers, even if a table claims PASS.
- Teardown: exit 0 only after owned observer workspace removal and named-instance stop, retaining transcripts. Never tear down prod/shared dev or silently substitute an occupied port.

Prior canonical receipt at ddc23c2ff: 22 controls PASS, original two red demos exit 1 for intended predicates; QA wrapper PASS 3/3, under /tmp/codeterm-qa/matrix-oracles-20261002-165719-79114. The54 controls from3906b3b46 and36 grouped corrections were independently verified with all166 at canonical c70a236c, receipt /tmp/codeterm-qa/probe-fixture-controls-20261002-185942-78176. No Worker gate ran;51 new owned-helper controls and all live cells remain NOT RUN; prior76 controls/five red demonstrations have their separate canonical84a5887f receipt.

No command is requested for Sentinel budget-exhausted/Jev-off: Brahms accepted N/A because regex classification never spends model budget. Exact authorization must name each command and promptmatrix:7794. Start/stop run in a Team-bound utility shell; no token enters argv. Until dependency source work completes these live commands are inventory, not runnable acceptance gates. Do not authorize the full batch yet.

Reviewed raw Sentinel schema is at ecb3973cdc7cb291058cc555bddbe4b336d0ea9a, ancestry-merged in canonical 0c93eebb94802918193d9339d541f7c6e6f6faaf. Fermi reports focused sentinel_ 28/28 and independent check-tests summary rc0 at /tmp/codeterm-qa/sentinel-check-tests-20261002-172516-71202. These source/mechanism gates do not qualify live cells. Disconnected absent IDs remain unavailable under zero-new-work and strict criteria remain PENDING owner reply to notification 1a74efb3-6446-46a0-b654-23e9091521d9. No new Auditor reads/IDs/journals or manufactured full opinions are requested. Sentinel cross-run comparison remains distinct and unapproved; static-modal/PTY approval and isolation are separate questions.

Passive source preparation: reviewed bfdde439a2ac717458570604f2cd15dd0cd072a3 remains UNMERGED behind held812c9f8f, qualification disabled. Seven emitted diagnostic point labels are recorded in contracts.json but not a universal covered-point list. Existing four provisional scrape_candidate commands remain PENDING; final approved binding must reconcile the actual emitted prompt_candidate label with commands/coverage together. observation_status=completed denotes parsed classifier consumption only. Distinct numeric capture ids do not prove distinct external events; skipped/unavailable alternates and update/onboarding safe-target evidence cannot populate a full heuristic opinion. Exact action-envelope join remains absent. No new gate or repeat feed regression requested. Canonical84a5887f independently passed the existing76 controls and all5 intended red predicates at /tmp/codeterm-qa/probe-binding-controls-20261002-173642-96464. The live inventory remains90 NOT RUN plus sole acceptedN/A.


## A-D continuation qualification commands — deferred, not authorized

After ancestry merge, the only immediate requested gate for this source checkpoint is:

```bash
python3 evals/prompt-matrix/oracle_tests.py
```

Expected exit0 and217 checks. The prior166 now have independent canonical c70a236c rc0 evidence, not Worker execution. The51 new controls add22 fake-process/event/clock/environment/handle and29 fake-helper/header/redirect/receipt/byte checks. The54 controls from3906b3b46 exercise native/platform/digest/archive escape/qualification, owner credential/home and cross-case rejection, event ordering/tree-before-barrier, and exact tool call/result/nonce/version receipts. The36 grouped controls add18 deterministic fake socket/clock/worker-completion checks,9 synthetic tar component/type checks and9 fake acquisition lifecycle checks. This gate opens no real socket, starts no real thread/process/network acquisition and executes no provider/installer. Five earlier intentional red commands above remain available with their exact failure predicates and prior canonical receipt; this checkpoint does not request a repeat feed gate.

The following exact macOS arm64 inputs are additional future qualification inventory. Each command requires separate explicit canonical authorization. None is approved to execute in this Worker lane. `--assemble` now invokes the source-authored owned acquisition helper and compares native bytes/layout/source; actual helper/platform qualification still requires source review and separate explicit authorization; `--verify-version` executes the real native CLI only. Neither authenticates a provider, proves an update prompt, enables the updater or passes a live cell. Run `--prepare` once first; preserve incomplete evidence rather than reusing an occupied fixture. Different Windows/Linux/x64 commands must be pinned to the runner's actual platform/libc/baseline before that runner is authorized, using the explicit layout table in README; no platform receipt is inferred from these macOS commands.

```bash
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider claude --version 2.1.286 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider claude --version 2.1.287 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider codex --version 0.159.3 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider codex --version 0.160.0 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider opencode --version 1.18.33 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider opencode --version 1.18.34 --platform darwin --arch arm64 --libc none --assemble
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider claude --version 2.1.286 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider claude --version 2.1.287 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider codex --version 0.159.3 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider codex --version 0.160.0 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider opencode --version 1.18.33 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrix --port 7794 --provider opencode --version 1.18.34 --verify-version
```

Historical default at015fc1307 refused before network/output with `Absolute-deadline registry transport unavailable; acquisition withheld`; no executed native red is claimed. Fermi source GO accepted that exact helper proposal. Current default assembly uses acquisition_transport/acquisition_worker sources. The first canonical macOS Claude2.1.286 wrapper attempt failed and remains unqualified; further commands require separate exact canonical authorization and failed-fixture preservation. ACQUISITION-TRANSPORT.md records the actual source boundary and known lifecycle limits. Assembly expects exit0 only with both archive integrity/digests, exact package/platform layout, matching installer source, contained extraction, native bytes and tree receipts. Native version qualification expects exit0 only with actual exact-version stdout, exit0 and the matching executable digest; it does not prove complete updater tree termination. Failures are retained and never replaced by a guessed layout or repeated acquisition. All live commands earlier remain interlocked PENDING before creation until actual native/auth/policy/trigger/updater/receipt binding and owner disposition. No updater-enabling command or invented confinement CLI is supplied. Courier's concrete boundary proposal is COURIER-CONFINEMENT.md, not a gate. The report command remains exit1 until all retained coverage and strict contracts qualify.


## Historical owned-helper pure gate — passed, no repeat requested

Previously authorized and executed in canonical2a18d9fa:

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-acquisition-demo
python3 evals/prompt-matrix/oracle_tests.py
```

The sabotage command must exit1 AND identify `Acquisition owned process termination unconfirmed`, because a finished flag is supplied without an observed process exit. The historical suite returned exit0 with217 controls at canonical2a18d9fa; its receipt is /tmp/codeterm-qa/probe-acquisition-controls-20261002-192817-29384. The acquisition red predicate also passed there; no repeat is requested. All process/thread/socket/network boundaries in these controls are fakes or mocked before invocation; local synthetic files only. Source pre-fix default predicate: `Absolute-deadline registry transport unavailable; acquisition withheld` at015fc1307. No executed pre-fix native acquisition result is inferred.

No actual helper/native command is requested as part of this pure gate. Future helper invocations occur only inside the twelve exact native assembly/version commands above, under separate canonical authorization. Parent/helper receipts alone do not qualify native tree/version, actual socket/process behavior, fixture auth, authentic update prompts or live cells. No repeat feed/broad/CI, updater opt-in or new live command is requested.

## Historical bounded TLS correction pure command — subsequently passed

After source review, ancestry-preserving merge and exact canonical authorization:

```bash
python3 evals/prompt-matrix/oracle_tests.py
```

Expected exit0 with270 controls (prior217 plus41 synthetic trust/context/read and12 sanitized failure-fact controls). New53 are source-authored NOT RUN. Every context/handshake/opener/process/OS CA source seam is fake; only synthetic local fixture files are read. No real bundle parse, process, socket, network or provider action qualifies. A supplied CERT_NONE context must be rejected by `TLS certificate/hostname verification weakened`; the pre-correction implicit context path lacked this explicit trust binding. A supplied nested URLError must retain its typed SSL verify_code instead of erasing the reason. These are source predicates, not executed new red receipts. No repeat red/feed/broad/CI gate is requested.

The old generic URLError receipt and incomplete native manifest stay untouched. Actual OS bundle parsing, HTTPS acquisition, cancellation, assembly and version commands are held for a separate exact disposition, with no retry or replacement authorized by this checkpoint. All owner/producer/Courier/native/trigger/Cursor/Windows and90 live requirements remain pending.

## Dedicated native version repair — deferred pure green

Later canonical bb699281 pure270 and all six corrected macOS archive assemblies have separate receipts recorded in RESULTS.md. The old failed promptmatrix fixture is preserved. No repeated acquisition/assembly, earlier red/feed/broad/CI gate or live command is requested.

Tests-first675202d was real-merged as canonical6e1df211. Its exact historical command was:

```bash
python3 evals/prompt-matrix/native_version_tests.py NativeVersionControls.test_legacy_receipt_is_refused
```

Canonical verified exit1, exactly1 selected and `AssertionError: FixtureError not raised`, without import/setup error. Receipt: /tmp/codeterm-qa/probe-version-red-20261002-200636-64415/full.log. This confirms the existing five-field receipt bypass. The rejection oracle is unchanged; no repeat red is requested.

After source review, real ancestry merge and separate exact canonical authorization, the sole requested green command is:

```bash
python3 evals/prompt-matrix/oracle_tests.py
```

Expected exit0,294 controls: prior270 plus24 dedicated version controls, with six existing NativeControls qualification fixtures strengthened and their rejection predicates retained. It uses fake process/event/clock/OS dependencies and synthetic local files only; no real thread/process/socket/network/native invocation. Actual Unix waitid/reap, Windows ABI/OVERLAPPED cancellation, creation and filesystem timing remain unqualified even if pure green passes.

## Proposed actual version inventory — held, not authorized

Only after review/merge/pure evidence, a host-exact separate disposition may authorize the first command. Stop on failure, retain the exclusive qualification home/sidecars and do not retry. Preserve/hash the old failed fixture and all six existing assembly inputs before/after; only version receipt metadata and new qualification evidence may change. First candidate:

```bash
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider claude --version 2.1.286 --verify-version
```

Remaining candidates each require a later exact authorization; the first command grants none of them:

```bash
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider claude --version 2.1.287 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider codex --version 0.159.3 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider codex --version 0.160.0 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider opencode --version 1.18.33 --verify-version
python3 evals/prompt-matrix/native_assembly.py --instance promptmatrixtls --port 7794 --provider opencode --version 1.18.34 --verify-version
```

Each expects exit0 only with schema2 request/resource/stream/retained-sidecar joins, immutable assembly inputs, exact one-version stdout, actual exit0/Unix sole reap or Windows captured-handle exit, clean EOF both streams, observer completion/join, owned handle closure and finalization before the original10s end. Any cancelled/overflow/error/late/unconfirmed fact refuses qualification. Receipt scope is direct_native_version_and_owned_handles and descendants=not_proven; it cannot prove updater process-tree confinement. These darwin/arm64 artifacts and commands provide no Linux/Windows proof. No installer, provider session, auth/model/daemon/live cell or updater opt-in is authorized.

## Cron launch-prompt matrix — canonical only, not authorized yet

This source defines 32 cells: provider × `owner-dir|new-dir|lane|home` × Jev `off|on`. The existing `trust-hidden` memory-worker cells remain in both arms in the original table. Cron provenance must bind the actual provider cwd to `provisioned_cwd`; only an authorized provider-declared trust answer may finish with `CronRunInfo.status=Success`. Missing provenance requires exactly one owner notice and zero action receipts. Every answer requires two distinct producer observation ids, its deciding arm, the real prompt transcript, the provider's emitted session marker, and an exact `CronRunInfo.session_id` match to the authenticated tab. Any non-trust prompt stays owner-pending with zero actions. The permission cell continues to require the same pending request and zero answers until its gate decides.

No live cell is authorized by this source merge alone. First merge the unattended-prompt producer, record the owner's explicit A/B/C isolation rule and stride, bind `launch_prompt_matrix.owner_decision` and `provider_auth_policy`, and qualify the provider target/auth route. The current `promptmatrix:7794` fixture already contains a preserved failed native acquisition; do not run `--prepare`, replace it, or reuse its native sidecar. The harness intentionally returns PENDING while those contracts remain open. All bench CLI traffic must continue through `dc`; never touch PROD 7685 or shared DEV 7686.

The existing duplicate-alert red must exit1 for the exactly-one-notice predicate. Each launch-prompt red must exit1 and print `REJECTED` naming its predicate.

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-demo
python3 evals/prompt-matrix/oracle_tests.py --red-launch-prompt-demo wrong-arm
python3 evals/prompt-matrix/oracle_tests.py --red-launch-prompt-demo duplicate-observation
python3 evals/prompt-matrix/oracle_tests.py --red-launch-prompt-demo missing-transcript
python3 evals/prompt-matrix/oracle_tests.py --red-launch-prompt-demo non-trust-key
python3 evals/prompt-matrix/oracle_tests.py --red-launch-prompt-demo unauthorized-off-answer
```

Then the complete pure validator suite must exit0 with305 controls (the prior294 plus eleven new launch-prompt/report controls):

```bash
python3 evals/prompt-matrix/oracle_tests.py
```

After source merge, owner choice, auth/native qualification and exact instance identity are confirmed, smoke the real named-instance CLI through the harness:

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --smoke
```

Run the 24 cells for Claude, Codex and OpenCode once each. Stop the loop on any failure; do not retry a cell with a new run id.

```bash
set -euo pipefail
for provider in claude codex opencode; do
  for folder in owner-dir new-dir lane home; do
    for jev in off on; do
      bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider "$provider" --case launch-prompt --folder "$folder" --jev "$jev"
    done
  done
done
```

Cursor remains NOT RUN until one real provider preflight proves it launches or records the exact unsupported response in `contracts.json::launch_prompt_matrix.providers.cursor`; an unsupported record must include its evidence path and reason. If it launches, run each of its eight folder/arm cells once with the same command shape. Do not infer support from a local archive.

Finally render the report, require exit0 only when every retained and launch-prompt executable cell passes, then stop the named instance while preserving all transcripts:

```bash
python3 evals/prompt-matrix/report.py --root target-check/evals/prompt-matrix/promptmatrix
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --stop
```
