# Canonical gate commands

Source authored only; this attempt executed none. These are concrete deferred command tokens, not authorization. Live provider creation is blocked by isolation disposition, final Auditor correlation, genuine disagreement provokers and confined executable/update bindings. Setup leaves provider updates disabled. The focused feed mechanism already passed within Sentinel28; no repeat feed regression is requested. Its live cell remains NOT RUN.

Run start/stop in a Team-bound utility shell; QA is serialized on the physical machine.

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
- Pure oracle suite: exit0,130 expected checks (76 previously verified plus54 new native/isolation/event/tool controls). Synthetic validator controls do not bind real producer fields.
- Focused Rust feed regression: already reported PASS within Sentinel28; no repeat requested. Historical exact command: `./build.sh test-one app supporter_sentinel_verdict_is_visible_in_activity_feed`. Retain original6029471427 source-expected-red checkpoint; no executed feed red is invented.
- `--smoke`: exit 0 only on real named-instance read/help responses, matching identity and canonical build SHA. Smoke proves no provider cell or transcript receipt. Provider observer creation additionally requires actual AgentReady, own session and resolved isolation.
- Every policy cell: exit 0 only on real prompt, correlated completed observations/paired arms and independent far end. Current unbound cells exit 3 before creation. Permission retains its request and shows zero answers/tool effect. Courier variants require one real install, safe Skip and own resumed-session version; busy requires install-before-idle-before-restart; failure requires zero restarts and one actual owner notice.
- Report: exit 1 until all 90 executable requirements plus any further Auditor points pass. Only budget-off N/A is accepted. All pending contract statuses and an empty/pending covered-point list are independent nonzero blockers, even if a table claims PASS.
- Teardown: exit 0 only after owned observer workspace removal and named-instance stop, retaining transcripts. Never tear down prod/shared dev or silently substitute an occupied port.

Prior canonical receipt at ddc23c2ff: 22 controls PASS, original two red demos exit 1 for intended predicates; QA wrapper PASS 3/3, under /tmp/codeterm-qa/matrix-oracles-20261002-165719-79114. New54 controls and live cells remain NOT RUN by this attempt; prior76 controls/five red demonstrations have their separate canonical84a5887f receipt.

No command is requested for Sentinel budget-exhausted/Jev-off: Brahms accepted N/A because regex classification never spends model budget. Exact authorization must name each command and promptmatrix:7794. Start/stop run in a Team-bound utility shell; no token enters argv. Until dependency source work completes these live commands are inventory, not runnable acceptance gates. Do not authorize the full batch yet.

Reviewed raw Sentinel schema is at ecb3973cdc7cb291058cc555bddbe4b336d0ea9a, ancestry-merged in canonical 0c93eebb94802918193d9339d541f7c6e6f6faaf. Fermi reports focused sentinel_ 28/28 and independent check-tests summary rc0 at /tmp/codeterm-qa/sentinel-check-tests-20261002-172516-71202. These source/mechanism gates do not qualify live cells. Disconnected absent IDs remain unavailable under zero-new-work and strict criteria remain PENDING owner reply to notification 1a74efb3-6446-46a0-b654-23e9091521d9. No new Auditor reads/IDs/journals or manufactured full opinions are requested. Sentinel cross-run comparison remains distinct and unapproved; static-modal/PTY approval and isolation are separate questions.

Passive source preparation: reviewed bfdde439a2ac717458570604f2cd15dd0cd072a3 remains UNMERGED behind held812c9f8f, qualification disabled. Seven emitted diagnostic point labels are recorded in contracts.json but not a universal covered-point list. Existing four provisional scrape_candidate commands remain PENDING; final approved binding must reconcile the actual emitted prompt_candidate label with commands/coverage together. observation_status=completed denotes parsed classifier consumption only. Distinct numeric capture ids do not prove distinct external events; skipped/unavailable alternates and update/onboarding safe-target evidence cannot populate a full heuristic opinion. Exact action-envelope join remains absent. No new gate or repeat feed regression requested. Canonical84a5887f independently passed the existing76 controls and all5 intended red predicates at /tmp/codeterm-qa/probe-binding-controls-20261002-173642-96464. The live inventory remains90 NOT RUN plus sole acceptedN/A.


## A-D continuation qualification commands — deferred, not authorized

After ancestry merge, the only immediate requested gate for this source checkpoint is:

```bash
python3 evals/prompt-matrix/oracle_tests.py
```

Expected exit0 and130 checks. The54 new controls exercise native/platform/digest/archive escape/qualification, owner credential/home and cross-case rejection, event ordering/tree-before-barrier, and exact tool call/result/nonce/version receipts. They execute no provider/network/installer. Five earlier intentional red commands above remain available with their exact failure predicates and prior canonical receipt; this checkpoint does not request a repeat feed gate.

The following exact macOS arm64 inputs are additional future qualification inventory. Each command requires separate explicit canonical authorization. None is approved to execute in this Worker lane. `--assemble` acquires native archives and compares bytes/layout/source; `--verify-version` executes the real native CLI only. Neither authenticates a provider, proves an update prompt, enables the updater or passes a live cell. Run `--prepare` once first; preserve incomplete evidence rather than reusing an occupied fixture. Different Windows/Linux/x64 commands must be pinned to the runner's actual platform/libc/baseline before that runner is authorized, using the explicit layout table in README; no platform receipt is inferred from these macOS commands.

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

Assembly expects exit0 only with both archive integrity/digests, exact package/platform layout, matching installer source, contained extraction, native bytes and tree receipts. Native version qualification expects exit0 only with actual exact-version stdout, exit0 and the matching executable digest; it does not prove complete updater tree termination. Failures are retained and never replaced by a guessed layout or repeated acquisition. All live commands earlier remain interlocked PENDING before creation until actual native/auth/policy/trigger/updater/receipt binding and owner disposition. No updater-enabling command or invented confinement CLI is supplied. Courier's concrete boundary proposal is COURIER-CONFINEMENT.md, not a gate. The report command remains exit1 until all retained coverage and strict contracts qualify.
