# dreaming-rem evals

Behavioural eval-set for `src-tauri/prompts/dreaming_rem.md` — the Memory
Optimizer (REM phase) prompt that runs as a periodic background worker
to clean up the mem graph (decompose hub-and-spoke nodes, bridge
disconnected islands, dedup entities).

## What's covered (5 cases)

Stage A.1 — Hub decomposition:

- **#1 red-zone-hub-not-skipped** — when a hub of degree ≥ 50 (RED ZONE)
  is in the input, the agent must NOT skip it; must decompose (full or
  partial) via `introduce-topic`. Was a real failure: dreamer left a
  71-edge hub completely untouched on one machine.
- **#2 biggest-hub-first** — when several hubs exist, the largest must
  be processed first (it concentrates the most search noise).
- **#3 partial-decomposition-allowed** — under tight budget on a
  RED-ZONE hub, the correct response is to extract 3–5 sub-clusters and
  log "partially decomposed" — not to skip the whole hub.
- **#4 homogeneous-low-degree-skip-ok** — a low-degree hub whose only
  edges are `extracted_from` provenance pointers is a valid SKIP, with
  reason "popular source, not a structural hub".

Stage A.2 — Island bridging:

- **#5 island-existing-anchor-relate-not-introduce** — when the
  `suggested_anchor` already exists in the graph, the agent must use a
  single `relate` edge, not a fresh `introduce-topic` (which is for
  inventing new themed anchors).

What's NOT covered yet (future): A.3 dedup pass, Stage B diary writing,
Stage C maintenance. Add cases as new failure modes show up.

## How to run

```bash
# Single-prompt sanity (current state of the prompt)
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/dreaming_rem.md \
  --eval-set evals/dreaming/evals.json \
  --runs 3

# A/B against the previous git revision
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-a HEAD~1:src-tauri/prompts/dreaming_rem.md \
  --prompt-b src-tauri/prompts/dreaming_rem.md \
  --eval-set evals/dreaming/evals.json \
  --runs 3

# Live dashboard at http://localhost:7779 while the run is in progress
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/dreaming_rem.md \
  --eval-set evals/dreaming/evals.json \
  --runs 3 --http-port 7779

# Single eval debug
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/dreaming_rem.md \
  --eval-set evals/dreaming/evals.json \
  --only-id 1 --runs 1 --verbose
```

A full 5-case × 3-run × 1-prompt sweep is ~75 LLM calls (15 sonnet +
~60 haiku judges).

## Sandbox model

Same as `general-agent/`: the agent runs in `--allowedTools ""` mode
and only describes commands it would issue. The eval prompts include
**mock graph-CLI output** so the agent has something concrete to react
to (it can't actually run `codeterm mem graph hubs`). Each expectation
is checked by a Haiku LLM judge.

When extending: keep the mock JSON realistic — fields the prompt
references (`active_degree`, `relation_types`, `sample_neighbors`,
`existing_anchors`, `suggested_anchor`) must be present and shaped like
the real CLI output, otherwise the agent's plan won't be testable.
