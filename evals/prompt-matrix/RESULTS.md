# Provider prompt matrix

Source phase only. No live matrix or red-green demonstration ran in this lane.

The 91 required cells below comprise 90 NOT RUN and one owner-accepted N/A (the original 86 executable cells plus four explicit disagreements). No transcript is claimed. The release gate remains closed.

Jev off is the heuristic arm. Jev on is the primary adapter arm with any actual fallback recorded. Deciding paths remain unobserved until journal-backed execution. Four explicit scrape_candidate disagreements are inventoried; Auditor's final covered-point list remains pending and may add coverage.

| Provider | Case | Variant | Jev off result / path | Jev on result / path | Transcript |
|---|---|---|---|---|---|
| claude | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | busy | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | fail | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | git | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | fake-ci | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | regex-no-jev | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | budget-exhausted | N/A: regex never spends model budget; owner accepted (Brahms, 2026-10-02) | NOT RUN / unobserved | none |
| host | sentinel | activity-feed | No off-arm cell: feed projection regression | NOT RUN / source repaired; live visibility unobserved | none |
| claude | permission | disagreement / scrape_candidate | No off-arm disagreement cell | NOT RUN / unobserved | none |
| codex | permission | disagreement / scrape_candidate | No off-arm disagreement cell | NOT RUN / unobserved | none |
| opencode | permission | disagreement / scrape_candidate | No off-arm disagreement cell | NOT RUN / unobserved | none |
| cursor | permission | disagreement / scrape_candidate | No off-arm disagreement cell | NOT RUN / unobserved | none |

Counts: PASS=0; FAIL=0; NOT RUN=90; N/A=1. The baseline 86 executable cells and sole N/A are preserved; four explicit disagreements are additional. Auditor's final point list may increase these counts.

Owner decision: budget-exhausted with Jev off is not applicable because regex mode never spends model budget. The Jev-on explicit cap plus a disconnected regex companion covers exhaustion. Source binding does not claim an executed outcome.

Source checkpoint: 6029471427 authored the feed regression as expected RED against the original exclusion; no executed feed failure was claimed. Source repair a97badc2d now accepts Sentinel rows, and Fermi reports the focused mechanism passed within Sentinel28; the real feed cell remains NOT RUN. Auditor correlation, Courier live provokers, provider isolation and update fixture binding remain PENDING.

Executed by this attempt: ACK, source/CLI-help inspection, git fetch/ancestry/diff review, registry metadata and wrapper archive digest inspection, local Cursor launcher/index/node hashes. No provider launched, prompt provoked, package installed, or harness/test/build/dev/live gate executed.

Prior independent canonical evidence: 22 controls PASS and two original red demos exit 1 at ddc23c2ff; receipt root /tmp/codeterm-qa/matrix-oracles-20261002-165719-79114. This attempt did not execute those commands. Their subsequent independent canonical76-control/five-red receipt is recorded below; no Worker execution is claimed. Source deltas and exact gates are in README.md and GATES.md. contracts.json records unbound dependency contracts.

Reviewed Sentinel raw schema ecb3973c is ancestry-merged in canonical 0c93eebb; mechanism/check-tests receipts are separate from this NOT RUN inventory. Disconnected absent observer IDs remain unavailable under zero-new-work. Owner notification 1a74efb3-6446-46a0-b654-23e9091521d9 is pending; strict criteria are retained, no paired-oracle adjustment or extra exemption is made. Source authoring adds 76 pure controls, all unexecuted by this Worker.

Independent canonical84a5887f receipt /tmp/codeterm-qa/probe-binding-controls-20261002-173642-96464 qualifies76 controls and5 exit1/intended-predicate red demonstrations only. Reviewed passive bfdde439 schema remains UNMERGED/PENDING behind held812c9f8f; classifier completion/capture identity is not external event/action evidence and no universal paired opinion is supplied. This metadata/docs preparation preserves90 live NOT RUN plus sole N/A, no new executable result.


Historical3906b3b46 checkpoint — accepted A-D source continuation: native platform/layout/integrity/tree assembly and separate version qualification sources, strict fixture-owned launch/home source, exact-cwd observer/worker scopes, and interlocked event-driven Courier phases are authored. No acquisition/install/--version/provider/daemon/gate was executed.54 new pure controls are authored and NOT RUN; combined expected suite count130 at unmerged3906b3b46. The prior76-control/five-red canonical receipt remains the only executed harness result. The narrow Courier confinement proposal is in COURIER-CONFINEMENT.md; no opt-in or daemon edit is made. Native qualification/authentic trigger/auth readiness, exact action/tool/tree/failure receipts, unmerged Auditor and owner decisions remain pending.90 live NOT RUN plus sole prior N/A remain unchanged.

Historical015fc1307 checkpoint — grouped review correction to3906b3b46: busy IO now uses one absolute deadline, bounded raw frames, explicit cancellation and a required finished event plus actual thread termination; connect/read/release receive no renewed budget. Archive spelling/type checks include implicit/explicit ancestors, allowing one same-spelled implicit-to-explicit directory declaration.36 additional deterministic controls are authored (18 deadline/lifecycle +9 synthetic tar collision/type +9 fake acquisition), for166 expected pure checks. No real sockets/network/threads/providers/gates ran in this lane. The unsafe urllib fallback is removed; default acquisition is withheld before network/output pending the exact bounded transport proposal in ACQUISITION-TRANSPORT.md. Fake transport completion is validator evidence only, and no native acquisition/qualification receipt exists. All90 live NOT RUN plus sole acceptedN/A and the prior76-control/five-red canonical receipt remain unchanged.

Fermi independent canonical c70a236c pure receipt /tmp/codeterm-qa/probe-fixture-controls-20261002-185942-78176 records python3 evals/prompt-matrix/oracle_tests.py rc0/166. This qualifies pure validator/fixture behavior only; no actual socket/process/helper/archive/provider/live behavior is inferred. Subsequent accepted source GO implements native_archive.acquire default dispatch plus acquisition_transport.py/acquisition_worker.py and51 new fake controls, for217 expected checks. The default-path control mocks the parent before launch. The helper’s bounded registry result is independently checked only after parent-observed process exit, handle closure and waiter termination; incomplete evidence/unconfirmed cleanup stays failed. New51 controls and all actual acquisition/assembly/--version/helper/provider actions remain NOT RUN. Cross-platform lifecycle limits are recorded in ACQUISITION-TRANSPORT.md; no guarantees are invented. All90 live NOT RUN, sole acceptedN/A, pending strict contracts and nonprod updater=false remain unchanged.
