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

A second source sweep found no honest frame for either missing label. The
provider fixtures hold no boot banner or spinner shown before the composer:
the Codex `model: loading` header in `composer-readiness` already shows the
composer. They also hold no repeated-error output. The `.cast` recordings
under `tests/fixtures/agent-state` start after boot, because capture is armed
once the agent is up. The two labels therefore come only from read-only dev
captures labelled by a person.

## Live captures

`capture_screen.py` reads one dev tab through the bench lib's `dc` wrapper.
It calls only `tab get-text` and `tab inspect`; it never sends input. Each
capture is scrubbed and held in the git-ignored `pending-captures.jsonl` with
`label: null`. A capture reaches `live-captures.jsonl` only when someone reads
the screen and assigns a label with `label --by`:

```bash
python3 evals/tab-state-jev/capture_screen.py capture --tab TAB --note "codex boot, no composer yet"
python3 evals/tab-state-jev/capture_screen.py pending
python3 evals/tab-state-jev/capture_screen.py label --id live-codex-1a2b3c4d-1 --label still_starting --by orchestrator
python3 evals/tab-state-jev/capture_screen.py drop --id live-codex-1a2b3c4d-2
python3 evals/tab-state-jev/prepare_fixtures.py
```

For `still_starting`, capture a freshly spawned dev agent before its composer
appears. Label only frames that show the provider's boot banner or spinner and
no composer. For `error_loop`, capture a dev agent that shows the same runtime
or tool error several times without making progress, for example one launched
against a model id the provider rejects. Drop any frame that does not clearly
show the state; never label one from the trigger alone.

Records in `live-captures.jsonl` carry `id`, `provider`, `boot_stage`, `label`,
`source`, and `screen`; `boot_stage_source` and `baselines` are optional.
`prepare_fixtures.py` merges them into the fixed screen set, keeps only the
final 40 lines, and scrubs paths, UUIDs, tab ids, and emails.

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
environment, or from `~/.codeterm-dev/supporter/typesafe-jev.json`. Use
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

`per_label_at_0_8` gives precision and recall per label at confidence 0.8;
recall uses the full dataset support, so a failed call counts as a miss.
`shadow_gate` pools precision at confidence 0.8 over the actionable set used
by the spawn-health shadow: auth, trust or permission, quota, update,
`still_starting`, and `error_loop`. It sets `default_on_allowed` only when every
one of those labels has at least 4 screens, no call failed, and the pooled
precision is at least 0.90. `[supporter.tab_state]` may ship enabled by
default only on that result.

Structural outputs are not recomputed with a reimplementation. They are
included only where a fixture-backed test directly gives an output for that
screen. Each comparison reports its coverage and Jev's score on the exact same
rows. The `boot_stage` comparison is a raw stage-by-screen-label table because
boot stages describe launch progress rather than the nine screen labels.

## Deferred oracle

The Orchestrator should run `python3 evals/tab-state-jev/run.py` once, record
the measured result and both use-case verdicts in plan section `sec_hgtyxy`,
and add dev captures if startup and error-loop coverage becomes available.
