# Tab-state Jev evaluation

This evaluation measures a screen-only prompt against a screen plus the context
the resident observer will send. screens.jsonl contains only real dev captures
whose label was assigned by a person after reading the captured screen.
Provider transport is copied from the provider capability descriptor.

## Capture and label

Use a named dev instance and the bench library. The capture helper is
read-only: it calls dc tab get-text, dc tab inspect, and dc agent providers.
It writes an unlabelled frame to the ignored pending-captures.jsonl; it never
chooses a label.

The context file supplies the event-time values known to the observer:

    {
      "requested_model": null,
      "session_model": null,
      "role": "Worker",
      "plan_id": "plan-id",
      "task_id": "task-id",
      "spawned_by_tab": "tab-id",
      "seconds_since_spawn": 12,
      "seconds_since_last_output": 4,
      "structural_verdict": null,
      "pending_prompt_kind": null
    }

The helper adds the provider capability descriptor and computes
screen_changed against the previous labelled capture for that tab. Keep the
context values aligned with the runtime request shape. Use these commands for
each capture:

    python3 evals/tab-state-jev/capture_screen.py capture --tab TAB --context-file CONTEXT.json --note "reproduction"
    python3 evals/tab-state-jev/capture_screen.py pending
    python3 evals/tab-state-jev/capture_screen.py label --id CAPTURE_ID --label HUMAN_LABEL --by orchestrator

The Orchestrator reads each screen and supplies its label. drop --id CAPTURE_ID
discards a frame that does not clearly show the target state. The label set is
ready_idle, working_normally, provider_update_prompt,
trust_or_permission_prompt, login_or_auth_required,
model_unavailable_or_quota, crashed_to_shell, error_loop, still_starting, and other.

Collect at least four frames per label pooled across providers and include both
`pty` and `chat_backend` transports. Reproduce a closed-port proxy, folder
trust and Codex update prompts, logged-out Codex or fresh Claude onboarding, an
invalid model, a killed provider, early startup, ready idle, and normal work.
Run prepare_fixtures.py after labeling; it sanitizes and writes the real
labelled records to screens.jsonl. Per-provider and per-transport precision
remain in the report but do not gate eligibility.

## Evaluate

Each invocation is capped at 300 Jev calls and makes one call per selected
frame. A token must be available as JEV_API_TOKEN, TYPESAFE_JEV_TOKEN, or
through --token-file for the named dev instance. Without an override, the
default token path is `~/.codeterm-dev/supporter/typesafe-jev.json`. The token
is never printed.

    python3 -m unittest discover -s evals/tab-state-jev -p "test_*.py"
    python3 evals/tab-state-jev/run.py --arm screen
    python3 evals/tab-state-jev/run.py --arm context
    python3 evals/tab-state-jev/run.py --arm context --criteria-variant auth-first-run

The screen arm sends the screen string as state. The context arm sends
state: {screen, context}. Each JSON report includes per-label precision and
recall, strong and weak band precision, per-provider and per-transport
precision, call counts, and a coverage-qualified verdict for each gate:

- Strong actionable predictions use confidence at least 0.80; precision must
  be at least 0.90.
- Weak predictions use confidence from 0.60 up to but excluding 0.80; overall
  precision must be at least 0.60.
- Lower confidence predictions are ignored by both gates.

The criteria variants compare the current auth criterion with a criterion that
includes first-run setup before sign-in. Record both context-arm reports. Save
the A/B winner's label map as criteria.json; Phase 2 embeds that file. An
incomplete set can produce exploratory measurements with `--allow-incomplete`,
but its gate is ineligible and cannot authorize runtime actions.

## Coverage policy

The default evaluator requires four pooled frames for every label and both
transports before any Jev call. `--allow-incomplete` permits interim
measurements for a partial set; reports retain missing labels/transports and
mark the gate ineligible. Use it to inspect exploratory results, never to
authorize runtime actions.


## Held-out recovery qualification

`heldout.jsonl` is intentionally empty until a human captures and labels new
frames. Never copy tuning frames or invent evidence. Collect at least four
frames each for provider_update_prompt, crashed_to_shell, still_starting and
error_loop, plus ready_idle, working_normally and other negatives. Include
trust/permission, login and unavailable-model examples to measure notification
precision. IDs and normalized screen hashes must be separate from screens.jsonl.

Use an existing named dev instance and its owned context file:

    CODETERM_INSTANCE=NAME python3 evals/tab-state-jev/capture_screen.py capture --tab TAB --context-file CONTEXT.json --note "held-out reproduction"
    python3 evals/tab-state-jev/capture_screen.py label --id CAPTURE_ID --label HUMAN_LABEL --by HUMAN_NAME --set heldout
    python3 evals/tab-state-jev/run.py --arm context --set heldout --token-file ~/.codeterm-dev-NAME/supporter/typesafe-jev.json

The recovery_qualification report measures automatic-label precision at 0.90
and notification-label precision at 0.80. A complete, separate dataset with at
least 16 automatic predictions and automatic precision at least 0.95 qualifies
for the live oracle. Missing coverage, errors, partial slices and lower precision
stay notify-only. Record the counts and model/adapter identity as a plan stride.

The committed qualification.json is disabled. Enabling it additionally requires
a successful live recovery oracle, the exact adapter/model and criteria hash,
the SHA-256 of the complete heldout.jsonl bytes, and the report's support and
correct/predicted counts. The runtime validates this receipt against its embedded
criteria and held-out bytes. Evaluation alone never enables actions. Confirmed exit-to-shell recovery is deterministic: the lifecycle event, process
absence and live shell authorize bounded resume or fresh launch. Jev is audit-only
for that event and does not gate or block recovery.
