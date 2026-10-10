# Slice 3: provider-bound Skip and delegated onboarding

## Observed source facts

Connected output candidates previously reached the native onboarding worker
before scrape classification. That event branch now uses the shared observer;
disconnected output retains the existing worker, and classifier fallback starts
the existing bounded-per-tab onboarding worker without waiting on it. The
deterministic launch-input gate and explicit prompt-handle path are unchanged.

Prompt candidates retain Q3 alongside Q1/Q2/Q4 in one adapter batch. Q3 contains
unique provider option values plus `other`. Redacted/truncated screens, sensitive
option values, ambiguous values and targets without a declared safe option omit
Q3. Unsolicited Q3 answers are discarded. Native Claude and its JS drop-in declare the exact narrow
onboarding choice with `launch_onboarding_safe_choice` / `launchOnboardingSafeChoice`.
The existing key-step response and disconnected parser output remain unchanged.

Update targets use the provider's safe option value, resolve its actual position,
and consume one provider-returned navigation/confirmation step. They never parse
the value as a numeric index. The provider must return explicit live-focus response steps; the generic native
arrow default cannot authorize automatic updates. Missing, throwing, malformed
or empty JS response hooks abstain without using that default.
The current dialog suffix must reproduce the full provider-parsed prompt before
the response builder sees it, so a historical focus marker cannot supply Enter.
Onboarding remains limited to delegated Workers and Planner Managers, including
post-launch Planners; owner-facing onboarding stays with the owner.

Automatic responses require >= 0.90 on every Q1/Q2/Q3/Q4 component, two distinct
completed observation IDs in the same occurrence and provider step, the same
adapter instance, model and policy, a matching current pending prompt, structural
modal evidence, and a qualification receipt for the new question/action schema.
Two actual captures may contain the identical static frame. Replayed or
out-of-order IDs, uncertainty, user response, adapter replacement, policy
changes and stale generations cannot reuse agreement. A model-classified prompt
is first published; its completed eligible observation requests one internal
confirmation capture through the bounded scheduler. That confirmation is
correlated to its parent and is not represented as new provider output. The new
receipt is shipped disabled; enabling automatic responses requires separately
verified held-out and live evidence, including onboarding-label coverage. No
live qualification receipt or metric is fabricated by this slice.

The action owner now permits one ticket per observation revision, while preserving
the prompt occurrence. Previously its claimed bit survived all revisions in an
occurrence, preventing a second event from reaching the response consumer.
Previous tickets become stale; existing ledgers still deduplicate response steps.
Onboarding shares the deterministic responder's exact focus-frame key, while a
new model occurrence may claim the same option again. Canceled cleanup checks its
own occurrence/frame and cannot release a newer owner. Update cleanup similarly
removes only its own claim.

The existing bound prompt-step writer rechecks the deadline, launch nonce/session,
PTY, screen, adapter, policy, observation and ledgers before consuming a step.
Its existing timing is unchanged. A queued step is not asserted to be a delivered
response: journal entries say `provider_safe_step_authorized` or
`provider_safe_step_stale`, retain `decided_by` / `observation_id`, and project
daemon-owned, scrubbed Q3 narrative through the Supporter feed.

No login, arbitrary affirmative answer, reset or provider installation is added.
Delivery/readiness/exit paths and their authority are untouched. No new sleeps,
polling, model-generated keys, provider-id branch, default request cap, or
lock-held model call is introduced.

## Authored coverage; no executed result

Tests were committed before implementation. `supporter_first_opinion` includes
authorization failures, minimum-component confidence, duplicate and
out-of-order observations, same-frame confirmation, expiry, adapter replacement,
occurrence/step reuse, option order/focus, historical-focus isolation, opaque
option values, unsupported defaults, fixture-only qualification, shared
deterministic/model ledger ownership and canceled cleanup. The Windows-only
`supporter_first_opinion_safe_response_qualified_static_onboarding_planner_completion`
uses the real candidate/model/publication/capture path and unchanged bound
responder; its child reports completion only after receiving the provider's
focused safe-option steps. It checks repeated identical captured screens and
correlated seed/confirmation/action records. Existing ServiceFixture cases retain
the owner/Planner boundary, user response, reused PTY/new nonce, disconnect and
expiration controls. These tests have not run in the source-only lane.

These tests have not run in the source-only lane. Standalone rustfmt was used as
the explicitly authorized source-editing operation. Compilation, runtime behavior,
Windows desktop cases and the far end of a qualified response remain unverified
until canonical QA and Probe execute the requested gates.

## Gates requested

Canonical must first regenerate the Claude bundle from authored TS, then commit
the generated artifact if it changes. The worker did not build or hand-edit it:

```sh
node scripts/build-plugin.mjs provider-claude
```

After real merge, run serially per machine; zero matched tests is a failure:

```sh
./build.sh check-tests
./build.sh test-one app supporter_first_opinion
./build.sh test-one app tab_state
./build.sh test-one app state_reactor
./build.sh test-one app trust_candidate
./build.sh test-one app manual_update
./build.sh test-one app trust_responder_never_writes_on_a_non_trust_dialog
./build.sh test-one app ga_connected_update_prompt_does_not_bypass_model_gate
./build.sh test-one app codex_update_dialog_is_a_respondable_structured_prompt
./build.sh test-one app launch_onboarding_unchanged_focus_redraw_does_not_queue_second_key
./build.sh test-one app post_launch_idle_planner_answers_onboarding_and_owner_prompt_is_not_auto_answered
./build.sh test-one app supporter_
./build.sh test-one app supporter_feed
./build.sh test-one app supporter_journal
./build.sh test-one app heartbeat_gate
./build.sh test-one app sentinel_connected_hash_gate_shared_budget_and_journal
./build.sh test-one app sentinel_budget_exhausted_no_route_journaled
./build.sh test-one app ga_screen_only_evidence_never_restarts_live_pty
./build.sh test-one app ga_unqualified_heldout_degrades_automatic_rows_to_notify_only
./build.sh test-one plugin parity_decision_model
./build.sh test-one plugin decision_physical
./build.sh test-one mem typesafe_physical_request_missing_token_does_not_count
./build.sh test-one mem typesafe_returns_usage_with_one_multi_request_and_credit_errors
./build.sh test-one plugin claude_onboarding
./build.sh test-one plugin js_runtime::parity_test
bash scripts/plugin-tests.sh provider-claude
bash scripts/plugin-tests.sh provider-codex
bash scripts/plugin-tests.sh provider-opencode
bash -c 'python3 -m unittest discover -s evals/tab-state-jev -p "test_*.py"'
npx biome check packages/plugin-sdk/src/capabilities.ts plugins/provider-claude/src/plugin.ts plugins/provider-claude/src/prompts.ts plugins/provider-claude/tests/plugin.test.ts
./build.sh test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary
bash scripts/clippy-gate.sh
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
scripts/check-file-sizes.sh --fail
gh workflow run tests.yml --ref release/1.12.4
```

Expected observations: all filters match; compilation, focused cases, native/JS
capability parity, provider TS suites, evaluation tests, clippy, formatting, Biome
and source-size gates succeed. Windows must execute the ServiceFixture cases,
not count their off-Windows desktop ignore markers as proof. The new automatic
receipt remains disabled and no unqualified connected case dispatches input.

Probe binds both arms to `decided_by` and `observation_id`. Its qualified harness
must observe the provider's Skip selected and the update modal closed, delegated
onboarding progressing by provider-returned keys to completion, owner onboarding
remaining visible, new same-option occurrences working, and user-response/nonce
races producing no stale input. Queue counts alone do not prove these outcomes.

The report supplies a literal lane tip for the protected-path oracle; its output
must be empty:

```sh
git diff --stat origin/release/1.12.4...<lane-tip> -- src-tauri/src/service/pty_io src-tauri/src/service/chat_send_transaction.rs src-tauri/src/api/tab_routes/send.rs src-tauri/crates/codeterm-pty
```
