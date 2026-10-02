# Slice 1: observation and action ownership

This is source-authored coverage, pending canonical QA after a real merge. The
27-row inventory remains pinned to its verified base in `INVENTORY.md`.

## Source changes

A connected observation captures a local launch identity, rendered frame,
prompt occurrence, exact adapter instance and an observation ID. Nonces,
sessions, PTYs, policy, exact prompt contents and ledger receipts stay local.
Screen and context sent to the adapter are redacted. The screen is bounded to
40 lines and 8 KiB; altered or truncated options cannot enable safe-choice
responses.

The scheduler has four workers, one active observation per tab, six named
questions per event, and 32 queued events. New noncritical frames coalesce
queued frames for that tab. Critical exit audits can displace noncritical
queued work. Dropped work is journaled; owner input never enters this queue.
There are no sleeps or polling. A captured exit uses a read-only episode permit
and cannot reset a newer launch's episode.

`DecisionModel::supports_question_batch` defaults to false. Native TypeSafe
explicitly supports it. A JS adapter must both implement `decideQuestions` and
return true from `supportsQuestionBatch`. The observer abstains otherwise.
Generic sequential adapters remain usable by other decision consumers.

Request accounting runs immediately before HTTP execute in native TypeSafe and
JS sync, async and stream fetches. Decision clients disable hidden protocol
retries and redirects; fetches outside a decision scope retain their client. JS workers inherit the accounting scope and
observation deadline. Local decisions consume no physical request. Observation
scopes refuse a second physical request even if the adapter hides that refusal.
An already depleted configured budget prevents model invocation; reservation
still occurs only at the transport boundary. There is no default hourly cap or
implicit four-call episode cap.

One action ticket owns model-driven responses. It compares the existing trust,
update, permission and Supporter ledgers, and the prompt detector's response
receipt. Supporter recovery still records the first verdict for agreement; it
requires the ticket before notifying or invoking a bound responder. Existing
local evidence and qualification checks continue to authorize actions. The
model supplies classifications and declared option values, never raw keys.

The only model wrapper constructor is `budgeted_model_with_activity`. Tab-state
observations supply `ActivityContext::TabState` with the daemon-owned title and
declared safe choice; the wrapper enforces their one-physical-request limit.
Narrator's daemon-owned question/answer entries accompany observer classification,
action and recovery records. Journal and feed projection preserve `asked`,
`decided_by` and `observation_id` together, while scrubbing and capping narratives.

Classification records carry `decided_by` and `observation_id`. Unsupported
adapters, errors, timeout, budget, other and low confidence are explicit fallback
arms. Queue and capture failures also receive IDs and safe journal records.
Adapter replacement/disconnection, user responses, stale sessions/nonces/PTYs,
policy changes and contradicted frames discard pending verdicts. Cancellation
of an old observation cannot invalidate a newer one.

The disconnected observer returns before screen capture, worker creation,
restoration reservation, ownership state or notification. Scheduler status does
not initialize its workers. Protected PTY delivery paths and owner-send
admission are unchanged.

## Slice boundary

This installs the shadow observation and shared ownership seam. Q4/Q5/Q6
question templates include prompt-candidate and anomaly contexts, covered through
metadata fixtures. The unused production trigger variants and heuristic-source
arm are deferred until slice 2 supplies their real callers. Routing provider/trust/
update scrape candidates through model-first arbitration is slice 2; safe Skip
and delegated onboarding are slice 3; unresolved auth/model/quota routes are
slice 4. Watcher/readiness authority is unchanged; slice 5 remains diagnostic
only. The existing start sleep, watchdog interval and detector cooldown are
untouched.

Two old assertions change intentionally: the implicit four-call episode cap
is removed, and a spawn verdict cannot survive replacement of its launch nonce.
Same-launch hook/output regression seams remain in the canonical gate list.

## Gates requested

Run serially in canonical after an ancestry-preserving merge; no zero-match
result counts as a pass. No build or test was run in the Worker lane.
Standalone rustfmt was explicitly authorized and used only to edit owned source.

1. `./build.sh check-tests` — compile all changed Rust interfaces and tests.
2. `./build.sh test-one app supporter_first_opinion` — all identity, cancellation,
   journal, privacy, scheduling, ownership, unlimited-budget and physical-call
   contracts pass. The five ServiceFixture cases must execute on Windows.
3. `./build.sh test-one app tab_state` — existing observer and metadata seams
   remain green, including healthy spawn, hook/output sequencing, budget,
   batching, distinct observations, stale generations and notification records.
4. `./build.sh test-one app supporter_` — shared-request spend, budgets, feed and
   existing supporter consumers remain green.
5. `./build.sh test-one app sentinel_connected_hash_gate_shared_budget_and_journal`
   and `./build.sh test-one app sentinel_budget_exhausted_no_route_journaled` —
   the existing Sentinel consumer accounts its simulated transport and never
   invokes a decision after its configured budget has been consumed.
6. `./build.sh test-one plugin parity_decision_model` — native/JS answer and
   explicit batch capability contracts agree; undeclared batching abstains.
7. `./build.sh test-one plugin decision_physical` — actual loopback HTTP calls
   inherit their scope; malformed requests cost zero; async deadlines propagate.
8. `./build.sh test-one mem typesafe_physical_request_missing_token_does_not_count`
   and `./build.sh test-one mem typesafe_returns_usage_with_one_multi_request_and_credit_errors`
   — missing credentials spend zero; each real batch/error transport spends one.
9. `./build.sh test-one app ga_connected_update_prompt_does_not_bypass_model_gate`,
   `./build.sh test-one app ga_screen_only_evidence_never_restarts_live_pty`, and
   `./build.sh test-one app ga_unqualified_heldout_degrades_automatic_rows_to_notify_only`
   — deterministic evidence and qualification still control automatic actions.
10. `./build.sh test-one plugin js_runtime::parity_test`, then
    `bash scripts/plugin-tests.sh provider-claude`,
    `bash scripts/plugin-tests.sh provider-codex`, and
    `bash scripts/plugin-tests.sh provider-opencode` — unchanged provider
    detection and responder contracts remain green. The source-verified module
    filter includes shared decision fixtures, Claude folder-trust fixtures, Codex
    update/trust separation and the bundled provider capability cases.
11. `bash -c 'python3 -m unittest discover -s evals/tab-state-jev -p "test_*.py"'`
    — criteria and evaluation mechanism remain green; no wording assertions.
12. `./build.sh test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`
    — the updated accounting documentation fits the embedded boundary.
13. `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`,
    `npx biome check packages/plugin-sdk/src/capabilities.ts`, and
    `scripts/check-file-sizes.sh --fail` — formatting and source boundaries pass.
14. `gh workflow run tests.yml --ref release/1.12.4` — Windows executes the
    ServiceFixture leg; macOS/Linux focused pure and adapter gates also pass.
15. `git diff --stat origin/release/1.12.4...<lane-tip> -- src-tauri/src/service/pty_io src-tauri/src/service/chat_send_transaction.rs src-tauri/src/api/tab_routes/send.rs src-tauri/crates/codeterm-pty`
    — output is empty. Use the submitted slice tip for `<lane-tip>`.

16. `./build.sh test-one app supporter_feed` and
    `./build.sh test-one app supporter_journal` — Narrator's grouped question/answer
    contract and the observation identity survive together through persistence
    and feed projection; private fields are scrubbed and narrative bounds hold.

17. `bash scripts/clippy-gate.sh` — production and test targets compile with
    warnings denied, including the observer and test-only completion handles.
18. `./build.sh test-one app heartbeat_gate` — Usher's gate and shared budget/feed
    cases pass. Its fixture declares its simulated physical transport, while
    the local-only and one-request observer contracts remain green under gate 2.

## Canonical bounce repairs

The first canonical run at `/tmp/codeterm-qa/auditor-20261002-145525-45496`
rejected slice 1. The missing-token fixture now checks that no observer starts,
instead of joining an absent handle. Async fetch coverage reads the direct
completion JSON and checks its body as well as HTTP status. Heartbeat's fixture
marks its simulated transport boundary; no request is charged at the model
wrapper boundary. This preserves zero-call local decisions and the observer's
single-request ceiling. `Action::Strong` retains its `strong_action` serialization.
The nonexistent `parity_heuristic` gate is replaced by the source-verified parity
module filter. These repairs remain source-authored until canonical verification.

Probe's live matrix is required after the classification routes land, with both
arms bound to `decided_by` and `observation_id`. This slice does not claim a live
provider journey or completion of the remaining four slices.
