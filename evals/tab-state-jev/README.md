# Jev terminal tab-state evaluation

This is a fixed, labeled screen set for measuring Jev's classification of
agent-tab terminal screens. `screens.jsonl` contains 60 records. Each record
stores the last 40 lines at most, provider, known `boot_stage` and its source,
human label, screen source, and detector output directly asserted by the cited
fixture test.
The fixture assembler is `prepare_fixtures.py`.

The corpus uses Codex, Claude, and OpenCode provider fixtures, plus screen
literals in `state_reactor` and `run_watcher` tests. Home paths, UUIDs, tab
ids, and email addresses are scrubbed from copied screens. Fixture text stays
otherwise unchanged.

## Coverage limits

The dev-only `dc tab list` read failed because the dev socket was not live in
this lane. No live tab screens were captured. The source inventory also has no
screen that can be labeled honestly as `error_loop` or `still_starting`; those
labels therefore have zero support. `boot_stage` is recorded only when a
fixture test ties a screen to a stage. The current set has one such record,
`provider_ready`, and cannot measure startup-stage agreement broadly.

Current support is: 31 trust or permission, 9 working, 7 ready idle, 5 quota,
3 update, 3 shell, and 2 auth screens. The trust-heavy mix is reported as
collected; the harness does not rebalance or resample it.

When read-only dev captures are available, add sanitized records to
`live-captures.jsonl` with `id`, `provider`, `boot_stage`, `label`, `source`,
and `screen`. `boot_stage_source` and `baselines` are optional. Then run
`python3 evals/tab-state-jev/prepare_fixtures.py` to merge them into the fixed
screen set. The assembler keeps only the final 40 lines and scrubs paths,
UUIDs, tab ids, and emails.

Several fixture screens intentionally share provider-specific classifier
tests. This corpus measures behavior on those real fixture frames; it is not a
held-out sample of independent production sessions. `run.py` reports absent
label support and returns `nothing` for use cases that lack complete state
coverage. Add read-only dev captures for startup and repeated-error states
before treating either use-case verdict as actionable.

## Run

The Orchestrator runs the live evaluation once from a parent environment with
network access and at most 300 Jev calls:

```bash
python3 evals/tab-state-jev/run.py
```

The script sends one `choice` request per screen to the TypeSafe Jev endpoint,
using model `jev-latest`. It does not update its question or criteria from the
predictions. The hard call ceiling defaults to 300 and cannot be raised.

The token is read from `JEV_API_TOKEN` (or `TYPESAFE_JEV_TOKEN`) in the parent
environment, or from `~/.codeterm-dev/decision-models/typesafe-jev.json`. Use
`--token-file PATH` for a named dev instance's token store. The script never
prints the token. `--endpoint` and `--model` can override the defaults; do not
use a production token store.

The JSON report includes per-label precision/recall, the confusion matrix,
macro accuracy (mean recall across represented labels), accuracy and coverage
at confidence 0.8, pooled and per-actionable-label precision at that threshold,
latency, detector comparisons on rows with directly recorded outputs, raw
`boot_stage` coverage, and one verdict for each use case. If Jev omits
`confidence`, the chosen-label probability is used and marked as its source.
Failed calls are counted and are never retried.

Structural outputs are not recomputed with a reimplementation. They are
included only where a fixture-backed test directly gives an output for that
screen. Each comparison reports its coverage and Jev's score on the exact same
rows. The `boot_stage` comparison is a raw stage-by-screen-label table because
boot stages describe launch progress rather than the nine screen labels.

## Deferred oracle

The Orchestrator should run `python3 evals/tab-state-jev/run.py` once, record
the measured result and both use-case verdicts in plan section `sec_hgtyxy`,
and add dev captures if startup and error-loop coverage becomes available.
