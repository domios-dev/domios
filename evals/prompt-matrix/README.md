# Isolated provider prompt matrix

A passing cell proves a real prompt/state, its exact launch/session/occurrence and decision point, two distinct completed observations, the recorded deciding arm and the provider or owner far end. A permission cell keeps the same request, records zero answers and leaves the requested tool effect absent. A route claim, authorized response, configured adapter or same-tab classification cannot substitute for those facts.

This continuation belongs to `pl_blazzd`, `COD-124` (`task_p7qsgp`). It is source-only. No provider, harness, test, build, dev server, smoke or live matrix ran in this lane. Canonical QA owns every executable gate after a real ancestry-preserving merge. `RESULTS.md` is an inventory.

## Isolation decision remains pending

Acceptance requires an isolated provider HOME, CODEX_HOME, CLAUDE_CONFIG_DIR and XDG roots for every case. The existing `provider_exec.py::main` restores the owner's roots for every non-login tab; the previous README attributed that to Brahms. These requirements conflict. Probe reported the exact discrepancy to Fermi before changing provider isolation. No credential was copied, auth file read, global hook rewritten or secret passed in argv. The wrapper is unchanged; `contracts.json::isolation` gates provider creation, including the observer, until the owner resolves the conflict and a conforming implementation is source-bound.

The daemon has a fresh HOME under `target-check/evals/prompt-matrix/promptmatrix/daemon-home`. Ports 7685 and 7686 are refused. All CLI operations use `dc` through `evals/_lib/lib.sh`; the named identity, loopback port, data directory and health `build_sha` must agree with canonical. No shared bench code changed. Setup now leaves `updater.allow_provider_updates_in_non_prod=false`: the old unconditional opt-in allowed the daemon's startup check to reach owner executables before an older isolated prefix existed. Courier opt-in remains pending verified updater confinement, then must use the real configuration/capability route. Current product source performs automatic provider/target admission without a consent click.

Canonical start/stop remain Team-bound utility-shell gates and machine QA stays serialized. The outer process group caps at 240 seconds; a cell caps at 180, CLI at 35, npm at 90, cleanup at 35. Failure deadlines never prove readiness. The collector waits on bounded SSE/probe/process events with no sleep or polling interval. Tokens arrive only on stdin. No harness or observer answers a provider prompt.

## Verified producer schemas at ddc23c2ff

The fetched release tip and lane base both equal `ddc23c2ff64ab1e1a2e075a5b5eafd2490f88382`. Existing source checkpoint `6029471427cd0f2359182c91e953d7ff10ee11a3` and feed repair `a97badc2d1523d25890747beb8bd55056f6e3ec6` are ancestors. No production source changed or historical attempt was reactivated.

| Producer | Actual emitted fields / behavior | Missing required proof |
|---|---|---|
| `service/supporter_tab_state/identity.rs::journal` | `time,kind=tab_state,tab,action=classification,decided_by,observation_id,label,confidence,reason,asked` | Decision point, launch/session, occurrence, completed/current observation and paired heuristic/adapter/selected verdict |
| `identity.rs::LocalIdentity::of_tab` | Private launch key, PTY, session, nonce, lifecycle and prompt hash; used for freshness | These private fields are not journal emission or an occurrence token visible to the tester |
| `observer.rs::record`, `candidates.rs::Pending::record` | Both call the same journal function; `candidates.rs::spawn` passes internal `scrape_candidate` to preparation | No emitted point discriminates candidate selection from observation/recovery; recorded classification can precede later stale/ticket checks |
| `provider_update.rs::start_update/install/journal` and `supporter/activity.rs::provider_update_action` | Run key `provider:target`; journal adds `provider,target_version,time` to `ActionRecord` (`kind,tab,label,confidence,action,reason,asked`) | Run correlation to exact prompt occurrence and independent safe-choice/session/version receipts |
| `provider_update.rs::record_safe_prompt_answer` | `label=safe_choice_answered`, tab/provider present, `target_version=null` | Launch/session/occurrence are absent; cannot invent target correlation for this row |
| `provider_update/state.rs::Coordinator`, `restarts.rs::readiness/claim_restart` | Install admission is independent of idle; restart only after success and eligible idle, no active turn/task; failure clears tabs; summary id persisted | Claims/resumed labels are insufficient without exact own-session new-version output and busy/idle barriers |
| `provider_update/storage.rs::provider_update_state_dir` | `HOME/.codeterm/provider-update-state.json`, not named dev data directory | Read only fixture daemon HOME; never owner state |
| `sentinel/scheduler.rs::record` | `time,kind,sentinel_id,tab,team_id,state_hash,label,confidence,action,reason,route,asked`; decision -> route_claim -> completed action | Lane baseline lacks attribution; reviewed ecb3973c raw schema is described below, separately from pending paired criteria and receipts |
| `supporter/feed.rs::entry` | Sentinel accepted; safe projection includes `time,team_id,kind,label,confidence_band,action,reason,budget_reached,fallback`, with conditional attribution/narrative | Visibility is an independent live gate, not policy provenance |

`DecisionSource::as_str` includes `adapter` and `fallback(no_adapter|timeout|error|budget|other|low_confidence)`. The previous validator omitted `no_adapter`; it is now mapped explicitly. `heuristic` remains a recognized journal vocabulary value, not evidence that this producer emitted a paired heuristic verdict. `observer.rs::spawn_observer` returns before preparation/journaling when disconnected, and `candidates.rs::spawn` returns on inactive/disconnected adapter. An enum spelling alone cannot prove a live no-adapter observation; The retained zero-new-work baseline makes disconnected observer IDs unavailable. Owner notification `1a74efb3-6446-46a0-b654-23e9091521d9` requests disposition of deterministic outcomes/receipts and actual same-question opinions; no change is approved. These cells remain PENDING. This is not a request for Auditor to add reads, IDs or journals. Connected predicates are actual partial decisions, not a manufactured complete heuristic verdict.

`contracts.json::decision.fields` contains nulls for missing fields. These record unmet retained criteria, never a fabricated producer schema or authorization to add producer work. `attribution.py` rejects incomplete binding even if somebody changes status to `bound`. It rejects wrong point/session/launch/occurrence, duplicate/stale ids, incomplete observations and selected-verdict/deciding-arm contradictions. Fixture field names in `binding_tests.py` are synthetic validator-seam proposals. They do not bind production or claim a real provider result.

## Provider cases and policy

Every provider (`claude,codex,opencode,cursor`) retains all nine cases in both Jev arms. A provider that never shows the requested real prompt does not pass. Cursor absence/unsupported behavior requires explicit owner disposition; none exists here.

| Case | Provoker | Bound source policy / remaining requirement |
|---|---|---|
| trust-hidden | Real memory worker in host-provisioned cwd | Existing trust responder; real trust prompt and provider-ready receipt required |
| trust-cron | Real owner-authored cron in its chosen cwd | `cron/spawn_spec.rs::build_cron_spawn_spec` now gives that cwd provenance; automatic trust answer expected |
| trust-visible | Real visible spawn in provisioned workspace | Existing trust responder; explicit own launch required |
| trust-lane | Git worktree in temporary repository, passed as cwd | No host-created lane provenance; one owner notification, no invented trust authority |
| update | Exact older executable in fixture prefix | Provider-declared safe choice expected; missing safe answer cannot switch the oracle to notify |
| login | Empty home/auth environment | One deduped owner notice; owner-facing prompt remains unanswered |
| permission | Ready agent requests one tool effect outside workspace | Same pending request, zero answers and zero file effect across two completed observations |
| bad-model | Wrapper selects invalid model after catalogue admission | Real provider failure and one owner notice; catalogue rejection is insufficient |
| crash | Unique provider descendant of own recorded launch | Own-provider signal and bounded outcome; Windows process-tree surface remains pending |

The trust journal's `reason=deterministic_directory_trust` is written before its input operation. It proves authorization, not commitment or delivery. Final source binding must correlate the outcome separately; this continuation changes no responder, delivery logic or delivery ledger. Update outcome expectations no longer depend on whether an answer was observed. The cron policy expectation is corrected using source commit `a9c1535d1`, not a product policy change.

## Courier controls and missing live provokers

`coordinator_oracles.py` independently requires one `update_started`, one ordered success/failure, exactly one observable summary whose id matches the fixture's persisted `summary_id`, one safe-choice receipt per real prompt occurrence, and one own-session launch/version receipt per resumed tab. Default rejects fresh launch as a substitute for required resumption. Busy requires install-complete while the same tab is still active, then own-session eligible idle before restart. Failure requires zero restart records/launches/version receipts and a completed post-failure barrier.

These validators return `OUTCOME VERIFIED` at the pure seam; the live judge also demands Auditor attribution before returning PASS. `Notification` does not serialize its internal event key, so the oracle uses persisted summary id and actual public notice fields instead of inventing an event-key receipt.

Real default/busy/fail provokers are still refused by `scenarios.py::pending_special`. Required remaining source work: confined real `codex update` capability invocation, isolated native prefix resolution and safe Skip receipt, event-signalled busy tool and eligible idle, independent `codex --version` tool result in each exact resumed session, and failure control in the daemon updater environment. Setting registry failure only in a provider-tab wrapper cannot fail the daemon's updater. No fake successful installer or persisted claim may replace the real command.

The confinement boundary is `provider_update.rs::install` -> `provider_update/command.rs::run_update_command/update_argv`: the daemon resolves the declared `codex update` through PATH and spawns it with inherited environment and null stdin. `fixtures.py::environment` supplies fixture HOME, npm prefix/cache and wrapper PATH; `provider_exec.py::main` otherwise resolves the owner's executable, selecting the older prefix only with a matching active update fixture. There is no proof that native `codex update` honors npm prefix or keeps its own installer destination inside that fixture. Before any opt-in, source must bind every resolver, native installer destination, child environment, target-version check and cleanup/termination boundary. The daemon command's 300-second failure deadline also exceeds the 180-second cell deadline: termination must be observed through the real owner boundary after a cell failure. No cap increase or product change is authorized here. Static-modal confirmation/PTY approval remains a separate pending question; no responder or delivery edit is part of this checkpoint.

## Exact candidate executable inputs

`versions.json` records exact registry metadata URL/SHA256, downloaded wrapper tarball SHA256 and verified SHA512 integrity, bin/scripts/optional dependency metadata and provider-manifest hash. Read-only archive inspection ran; no package was installed or executable launched.

| Provider | Older literal | Offered target observed | Prompt status |
|---|---|---|---|
| Claude | 2.1.286 | 2.1.287 | PENDING: native placeholder needs installation binding; update prompt unobserved |
| Codex | 0.159.3 | 0.160.0 | PENDING: provider launch/resume explicitly sets `check_for_update_on_startup=false` |
| OpenCode | 1.18.33 | 1.18.34 | PENDING: native placeholder needs installation binding; update prompt unobserved |

Sources are the version-specific [Claude metadata](https://registry.npmjs.org/@anthropic-ai%2Fclaude-code/2.1.286), [Codex metadata](https://registry.npmjs.org/@openai%2Fcodex/0.159.3), and [OpenCode metadata](https://registry.npmjs.org/opencode-ai/1.18.33). Package existence and a newer registry version never prove a real prompt. Claude/OpenCode postinstall copies a native optional-dependency binary over `bin/*.exe`; the existing `--ignore-scripts` install fixture is therefore unbound and refuses execution. All 52 referenced exact native distribution metadata records now pin source URL/hash and registry-declared tarball integrity, OS and CPU; native archive bytes are explicitly unverified. Native archive byte verification, platform-specific executable assembly and actual update prompt proof remain required. Codex's capability check reads a mutable registry tag; target drift must fail review against the recorded target before authorization.

Cursor exists at `/Users/kandrey/.local/bin/cursor-agent`, pointing to `versions/2026.07.01-41b2de7`. Archive `versions/2026.06.19-20-24-33-653a7fb` exists under `/Users/kandrey/.local/share/cursor-agent`. `versions.json` hashes launcher, index.js, bundled node and every file in both local version trees (177 archive files, 178 current files), with a canonical file-manifest digest; the launcher resolves and executes its own bundled node/index.js. The manifest has no updater capability. No archived launch, portable archive acquisition or supported real update prompt is proven. Cursor update stays PENDING; no coverage exemption was added.

## Sentinel and feed checkpoints

Git/fake-ci/regex/budget/feed provokers remain intact. Git requires the own shell creator/workspace notice. Fake CI requires named recipient receipts in its own bound session, one per transition. Nonce-bound probe process events and persisted unchanged state provide silence barriers; neither a claim nor elapsed time is a receipt. The regex companion remains deterministic during cap exhaustion. Exact prior adapter/cap values, including null for no cap, are restored.

Budget-exhausted/Jev-off is the sole N/A, accepted by Brahms `b8a1e869` on 2026-10-02 because disconnected regex never spends model budget. Nothing else is exempted. Both-arm Sentinel attribution remains pending.

Reviewed raw source `ecb3973cdc7cb291058cc555bddbe4b336d0ea9a` adds `sentinel/provenance.rs` and captures `sentinel_id,tab,team_id,workspace_id,state_hash,evaluation_id,observation_id,classification_source,classification_reason,label,confidence,stage,route_status,action,reason,route,route_target,recipient_receipt,asked,budget,time,kind`. The UUID `evaluation_id` is fresh per evaluation; `observation_id` equals it only for selected adapter/regex attempts, including refusal/error. It is not numeric Auditor identity, a completed response, or physical call accounting. `state_hash` hashes probe bytes plus exit; shared request JSON hashing is a different domain and cannot be joined by equality. Unchanged evaluations have no attempt, label or route. Failed probes have null hash. Failed routes retain `classification_reason=transition` while `reason=route_failed`. Routing API completion has `route_status=api_completed`, intended `route_target`, and always null `recipient_receipt`.

`sentinel_raw.py` validates the captured scope/hash/UUID across ordered stages. Outcome validation still requires independent call counts and recipient own-session/public-notification receipts. `sentinel_scenarios.py` captures scope from the real caller-scoped show response, never a same-tab guess. The raw classification source is separate from any paired heuristic/adapter opinions; none are emitted or filled in here. Sentinel same-event criteria remain PENDING owner disposition; a future cross-run comparison is not silently substituted. Public feed projection removes raw identifiers and accepts numeric Auditor observation ids only; it cannot establish raw Sentinel UUID correlation.

Canonical merge `0c93eebb94802918193d9339d541f7c6e6f6faaf` ancestry includes ecb3973c, verified read-only. Fermi reports focused `sentinel_` passed 28/28 including seven new mechanism tests and the feed regression. The independent check-tests summary at `/tmp/codeterm-qa/sentinel-check-tests-20261002-172516-71202/summary.txt` records rc0. No Worker gate ran; no live recipient acceptance is inferred. The lane remains based on ddc23c2ff and references this source explicitly; integration must retain the real merge ancestry.

Original checkpoint: `6029471427` authored the feed regression against a source exclusion and described it as expected RED; it did not execute that regression. Current source repair `a97badc2d` accepts Sentinel in `feed.rs::entry`. The focused persisted Team-scoped regression and real activity-feed/on cell now expect success, but are NOT RUN. Preserve any independent original red receipt if canonical QA has one; do not invent it or delete the checkpoint.

Prior independent oracle evidence at canonical `ddc23c2ff`: 22 controls passed; duplicate owner notice and duplicate Sentinel transition demos exited 1 for their intended predicates. Receipt root: `/tmp/codeterm-qa/matrix-oracles-20261002-165719-79114`. This is previous canonical evidence, not execution by this attempt. The updated suite authors 76 controls (22 prior + 18 attribution + 15 Courier + 5 report + 16 Sentinel raw controls). Five intentional red commands and the new control count remain deferred.

## Coverage and canonical gates

The original 86 executable cells plus one accepted N/A remain. Four additional explicit disagreement requirements exercise the real internal `scrape_candidate` selection point with permission prompts, one per provider. Their emitted schema and genuine differing-arm provocation remain PENDING. Auditor's authoritative covered-point list is still absent; final coverage unions that list with these four requirements and never shrinks the baseline. `GATES.md` contains concrete commands, with candidate older literals and no unexpanded version variables.

`report.py` reads every required cell, rechecks PASS using the authoritative checked-in contract and independent oracle, selects the newest attempt per full cell key, and rejects missing/pending/failed outcomes. An empty covered-point list cannot green the report. Pending isolation, update fixture, coordinator, Sentinel or decision binding keeps exit nonzero even if a result table claims all PASS. The inventory is at least 91 cells: 90 NOT RUN plus one accepted N/A; final Auditor points may increase it.

Source authoring does not satisfy final acceptance. Fermi must resolve retained oracle criteria and isolation, land remaining approved source slices, then authorize a separate evidence-only runner in canonical. That runner proves named-instance CLI smoke/readiness, each real prompt, its paired deciding arms and far-end outcome, report exit 0 and cleanup. Every executed finding must be preserved red-first and repaired only by its implementation owner.
