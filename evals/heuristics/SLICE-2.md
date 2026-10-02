# Slice 2: live prompts and scrape candidates

## Observed source facts

The output path used to invoke the directory-trust responder before resolving a
structured handle. Update candidates used a separate classifier thread. The
parked reactor classified provider scrape candidates independently.

Connected scrape candidates now use the existing observation envelope and bounded
scheduler. The batch contains Q1 `prompt_visible`, Q2 `label` and Q4 `input_area`;
its criteria hash covers the actual named question set. The model decides above
the 0.60 minimum component floor unless a choice is `other`. Timeout, budget,
unsupported primitives, malformed answers and connection errors abstain into the
existing heuristic resolver. A disconnect or stale capture cancels the consumer.
A negative model classification does not run the heuristic parser as a veto.

The worker records `decided_by` and `observation_id`, retains the one-physical-call
wrapper, and claims the shared action ticket. It checks the captured screen,
launch identity, exact adapter, policy and ledgers before consuming the result.
Publishing rechecks that ticket after local provider parsing. Structured handles
and explicit detector receipts bypass the model. The disconnected output and
reactor branches retain their existing parser/responder path.

Publishing a model-classified prompt requires provider-parsed options, the existing
structural footer/focus evidence, and the composer-copy negative guard. It requires
user confirmation; model labels never supply raw keys or automatic approvals.
Owner routing requires confidence >= 0.80. Existing heuristic responders remain
the fallback. Safe Skip/onboarding changes belong to slice 3.

Startup escalation also asks Q4. Composer, historical/draft, `other`, or weak input
area evidence suppresses prompt liveness used by recovery. Launch input, delivery,
readiness, exit authority, the reactor sleep and detector cooldown are untouched.
No provider capability or provider-id branch is added. The older requested
`codex_update_dialog_resolves_to_skip_with_one_notification` test name is absent
at this release tip; the existing respondable-prompt test and manual-update
module filter below replace that zero-match command.

## Supporter connection integration

The connection contract uses `SupporterConfig::active_id()`. Candidate capture
returns before screen reads or queue admission when no active ID exists, even
when saved connections and registered adapters remain. It uses the shared
configured-model check and captures the active ID for every worker revalidation.
The observation policy continues to include only the active connection's saved
model and endpoint. No adapter is chosen as a fallback.

## Authored coverage; no executed result

`supporter_first_opinion` includes scrape selection and every fallback arm,
no-adapter zero reads/calls/consumer work, redaction, one batch, journal IDs and
sources, stale captured frames, replacement generations, disconnect cancellation,
unknown launch-block input-area evidence, and full transcript/composer negatives.
Connection cases assert zero work with absent/empty/unregistered active IDs and
abstention when the active saved model has not been applied.
ServiceFixture cases assert the persisted prompt, explicit-handle precedence,
failed revalidation, user-response cancellation without a redraw, reused PTY/new
nonce cancellation, and zero prompt-step dispatches.

These tests have not run in the isolated lane. Runtime parity and Windows desktop
behaviour remain hypotheses until canonical QA executes the commands below.
Standalone rustfmt was used only as the authorized source-editing operation.

## Gates requested

Run each gate serially per machine after a real merge. Zero matched tests is a
failure. Linux/macOS exercise the pure cases; the Windows matrix must execute the
ServiceFixture cases rather than accepting their desktop-runtime ignore markers.

```sh
./build.sh check-tests
./build.sh test-one app supporter_first_opinion
./build.sh test-one app tab_state_
./build.sh test-one app state_reactor
./build.sh test-one app trust_candidate
./build.sh test-one app manual_update
./build.sh test-one app trust_responder_never_writes_on_a_non_trust_dialog
./build.sh test-one app ga_connected_update_prompt_does_not_bypass_model_gate
./build.sh test-one app codex_update_dialog_is_a_respondable_structured_prompt
./build.sh test-one app launch_onboarding_unchanged_focus_redraw_does_not_queue_second_key
./build.sh test-one app post_launch_idle_planner_answers_onboarding_and_owner_prompt_is_not_auto_answered
./build.sh test-one app supporter_feed
./build.sh test-one app supporter_journal
./build.sh test-one app heartbeat_gate
./build.sh test-one plugin js_runtime::parity_test
bash scripts/plugin-tests.sh provider-claude
bash scripts/plugin-tests.sh provider-codex
bash scripts/plugin-tests.sh provider-opencode
bash -c 'python3 -m unittest discover -s evals/tab-state-jev -p "test_*.py"'
bash scripts/clippy-gate.sh
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
scripts/check-file-sizes.sh --fail
gh workflow run tests.yml --ref release/1.12.4
```

Expected observations: compilation and clippy succeed without allow attributes;
all selected tests match and pass; provider capability parity remains green;
formatting and source-size gates pass. Probe's live matrix must bind both model
and fallback arms to the decision source and unique observation ID, show no
pending prompt for transcript/composer copies, preserve explicit handles, and
show no input writes from model classification.

The report supplies the literal lane-tip SHA for this protected-path oracle;
its output must be empty:

```sh
git diff --stat origin/release/1.12.4...<lane-tip> -- src-tauri/src/service/pty_io src-tauri/src/service/chat_send_transaction.rs src-tauri/src/api/tab_routes/send.rs src-tauri/crates/codeterm-pty
```
