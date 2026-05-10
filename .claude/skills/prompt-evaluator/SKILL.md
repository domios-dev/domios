---
name: prompt-evaluator
description: Evaluate, A/B compare, or check for regression in any CodeTerm prompt file (general_agent.md, worker_suffix.md, lead_suffix.md, codeterm_rule.md, agent definitions in .claude/agents/, bridge rules, etc). Triggers when the user asks to "test the prompt", "run evals", "check for regression", "did my prompt change break anything", or "compare prompt before and after". Runs `runner.py` against the matching eval set in `evals/<name>/`.
version: 0.1.0
---

# Prompt Evaluator

CodeTerm has multiple system prompts and instruction files that drive agent behavior. When you change one of them, you need to know whether the change preserves the behaviors the prompt is supposed to elicit. This skill is the harness for that.

## When to use

- Changed `src-tauri/prompts/general_agent.md` and want to verify the General Agent still routes Telegram, spawns the right roles, never echoes after `bridge send`, etc.
- Compressed a prompt to save tokens — run A/B against the previous git revision to confirm no regression.
- Adding a new behavioral rule to a prompt — write an eval first, run it against the unchanged prompt to confirm it currently FAILS, then add the rule, run again to confirm it now PASSES.
- Reviewing a PR that touches any file in `src-tauri/prompts/` or `.claude/agents/`.
- Sanity-checking that a refactor of `worker_suffix.md` / `lead_suffix.md` / `codeterm_rule.md` didn't strip a load-bearing instruction.

## How it works

The runner spawns the local `claude` CLI in **non-interactive sandbox mode** for each eval prompt:

```bash
claude -p "<eval prompt>" \
  --append-system-prompt "$(cat <prompt-under-test>)" \
  --allowedTools "" \
  --output-format json \
  --model sonnet
```

- `--append-system-prompt` injects the prompt-under-test on top of Claude Code's default system prompt. The full instruction set under test is what the agent sees.
- `--allowedTools ""` is **critical**: the agent has zero tools available. It cannot run `Bash`, cannot read files, cannot write — it can only respond in text describing what it WOULD do. This makes evals safe (no real side effects on your machine, no API calls, no fs writes) and parseable (the agent's plan is in plain text, not hidden in tool calls).
- `--output-format json` returns a structured envelope; we read `result.result` for the agent's reply.
- Each eval is run **3 times** by default to catch non-determinism. 3/3 = ✓, 2/3 = ⚠ flaky, 0/3 = ✗ broken.

For each eval, every expectation in `expectations[]` is checked by an **LLM judge** — a second `claude -p` call (Haiku by default) that asks "did the response satisfy this expectation? PASS or FAIL". Substring/regex heuristics were considered and dropped: they create false positives (tokens appearing in unrelated context, paraphrased commands, near-misses on negative constraints) and the cost saving over Haiku is negligible. One uniform path is simpler and more reliable.

The judge prompt is strict: it explicitly tells the judge to FAIL if the agent only vaguely mentions a required behavior without committing to it, if a required CLI flag/path/file is absent, or if a negative constraint is violated.

## How to run

### Single prompt eval (just check current state)

```bash
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/general_agent.md \
  --eval-set evals/general-agent/evals.json \
  --runs 3
```

Output: a markdown table to stdout with pass/fail per eval. Exit code `0` if all pass, `1` if any fail.

### A/B compare two prompt versions

```bash
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-a "HEAD~1:src-tauri/prompts/general_agent.md" \
  --prompt-b "src-tauri/prompts/general_agent.md" \
  --eval-set evals/general-agent/evals.json \
  --runs 3 \
  --report markdown
```

`--prompt-a` accepts either a file path or a `<git-revision>:<path>` reference (resolved via `git show`). The runner will fetch the old version from git without touching the working tree.

Output: side-by-side table with `Δ` column showing regressions (`Δ < 0`), improvements (`Δ > 0`), and unchanged. For each regressed eval, the runner prints which expectations now fail and the agent's response that broke them.

### Save report to file

```bash
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-a HEAD~1:src-tauri/prompts/general_agent.md \
  --prompt-b src-tauri/prompts/general_agent.md \
  --eval-set evals/general-agent/evals.json \
  --report markdown > .claude/skills/prompt-evaluator/reports/general-agent-2026-04-07.md
```

Reports directory is gitignored — they're per-machine artifacts.

### Live dashboard (long runs)

For long `--thorough` A/B runs the user often wants to *watch* progress. Add `--http-port <N>` and the runner spawns a tiny in-process HTTP server (pure stdlib — no Flask/Tornado) that serves a single-page dashboard:

```bash
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-a HEAD~1:src-tauri/prompts/general_agent.md \
  --prompt-b src-tauri/prompts/general_agent.md \
  --eval-set evals/general-agent/evals.json \
  --runs 3 \
  --http-port 7779
```

The page (default browser opens automatically; pass `--no-open` to suppress) polls `/state.json` ~every 1.5s and renders:

- Top progress bar — calls done / total + ✓/⚠/✗ counts
- Per-eval row with status pill, expand to see prompt + every run's response + per-expectation matcher verdicts (pass/fail + reason)
- Final markdown report embedded at the bottom on completion

The HTTP server stays up after the run finishes so you can browse the final state. `Ctrl-C` to stop.

Pick a free port — `7685` (prod CodeTerm) and `7686` (dev) are taken; `7779` / `7780` are good defaults.

## How to interpret a report

```
# Eval report — general_agent.md

| ID | Eval                              | A (HEAD~1) | B (HEAD)  | Δ  |
|----|-----------------------------------|------------|-----------|----|
|  1 | implicit-request-ask-not-act      | 3/3 ✓      | 3/3 ✓     |  0 |
|  2 | explicit-request-create-workspace | 3/3 ✓      | 2/3 ⚠     | -1 |
|  8 | isolation-notes-storage           | 3/3 ✓      | 0/3 ✗     | -3 |
```

Reading rules:

- **`Δ = 0`** — same pass rate. Prompt change is neutral for this eval.
- **`Δ > 0`** — prompt change *improved* this eval. Good.
- **`Δ < 0`** — prompt change *weakened* this eval. **Investigate before merging.**
- **`A = 3/3` and `B = 0/3`** with `Δ = -3` — definitely broken. The new prompt removed something this eval depends on.
- **`A = 3/3` and `B = 2/3`** with `Δ = -1` — likely flakiness from the change, worth one more 3-run pass before deciding.
- **`A = 2/3` and `B = 2/3`** with `Δ = 0` but both flaky — pre-existing flakiness, probably the eval is poorly worded; rewrite the eval, not the prompt.

For each `Δ < 0` row the runner prints a follow-up section showing:
- The expectations that newly failed
- A diff snippet showing what's missing in the agent's response (compared to passing runs in A)
- A guess at which prompt section was over-compressed / mis-edited

Use that to decide: `git revert <commit>` the prompt change, OR cherry-pick a single section back, OR rewrite the eval if it turns out the eval was demanding something the prompt should never have promised.

## How to add a new eval set

Eval sets live in `evals/<name>/evals.json`. The shape is:

```json
{
  "skill_name": "general-agent-prompt",
  "description": "What this set covers",
  "evals": [
    {
      "id": 1,
      "name": "short-kebab-case-name",
      "prompt": "User input the agent will receive (with prefixes like [Telegram: ID] if relevant)",
      "expected_output": "One-sentence summary of correct behavior — used in reports",
      "expectations": [
        "Uses send-text --pane general --mesh PEER",
        "Does NOT use workspace create",
        "Includes 'report back' phrase"
      ]
    }
  ]
}
```

Tips for writing good evals:

- **One eval = one behavior.** Don't bundle "spawns the right role AND uses the right CLI flags AND replies via the right channel" into one eval — make three.
- **Expectations are independent assertions, not steps.** Each one is checked separately; an eval passes only if ALL expectations pass.
- **Use technical expectations whenever possible** — they're matched by regex without an LLM call, so they're free and deterministic. Reserve behavioral expectations (LLM-as-judge) for things you genuinely can't grep for.
- **Negative expectations matter.** "Does NOT use workspace create" is just as important as "Uses send-text". They prevent regressions in the *other* direction.
- **Test the eval first** by running it against the *unchanged* prompt and confirming it passes. If it fails on the unchanged prompt, the eval is wrong, not the prompt.
- **Eval prompts should be realistic** — copy real user messages from `~/.codeterm-dev/general/MEMORY.md` daily notes or actual logs, don't synthesize "test inputs".

## Existing eval sets

- `evals/general-agent/evals.json` — 21 evals covering message routing (Telegram/mesh prefix), role selection, mesh delegation, recon-before-action, persistent memory protocol, mono-repo workspace handling, photo replies via Telegram bridge, identity wake-up.
- `evals/mem-protocol/evals.json` — 8 evals for the slim `bridge_mem.md` rule shipped to non-GA agents (remember-now, search-before-answering, identity-via-wake-up, no-id-guessing, English-in-mem, sessions-vs-mem-routing, no side-store writes).
- `evals/dreaming/evals.json` — 5 evals for the REM (dreaming) prompt — Stage A.1 hub decomposition with RED ZONE rule + Stage A.2 island bridging.

When you write a new eval set, drop a one-line entry in this list.

## Limitations

1. **Sandbox mode means the agent can't actually call CodeTerm.** It describes what it would do in plain text; the judge reads that description. If a prompt's behavior depends on the agent reading actual files or pane state, this harness can't test it. For those cases, write an integration test in `tests/e2e/` instead.
2. **LLM-as-judge is not perfectly deterministic.** Even the same expectation+response pair can flip verdicts run-to-run (~5% based on Anthropic eval research). Use `--thorough` (3 runs) when you need higher confidence. The default 1 run is fast for routine "did I just break something" checks where one verdict is enough.
3. **Uses your Claude Code auth.** Runs through whatever `claude login` is currently active — OAuth subscription or API key, whichever you have. Use `--dry-run` first to see the call count if you want to budget against subscription quota. Typical default `A/B quick` run is ~100 calls (Haiku-mostly), `--thorough` triples it.
4. **The prompt under test is *appended* to Claude Code's default system prompt, not replacing it.** This matches real usage (CodeTerm prompts always run on top of Claude's base behavior), but it means you can't test prompts that contradict Claude defaults. If you need a clean slate, hack `runner.py` to use `--system-prompt` instead of `--append-system-prompt`.
5. **Git-revision references require a clean working tree** for the file under test. If you have uncommitted changes to `general_agent.md` and ask for `HEAD~1:general_agent.md`, you get the committed version, not your working copy — which is what you want for A/B but might confuse if you forgot.
6. **`--runs` defaults to 1.** That's a deliberate trade-off vs skill-creator's 3. With the sandboxed agent and a strict judge prompt, single runs are stable enough for most "did I break it" checks. Use `--thorough` (which sets runs=3) for the rare cases where the eval seems flaky or you need pre-merge confidence.

## Implementation files

- `runner.py` — orchestration, CLI argument parsing, A/B aggregation, report rendering
- `matcher.py` — hybrid expectation matcher (regex + LLM-as-judge)
- `dashboard.py` + `dashboard.html` — optional in-process HTTP server for `--http-port`
- `reports/` — gitignored output directory

To debug a single eval without rerunning the whole set: `python runner.py --prompt-file ... --eval-set ... --only-id 8 --runs 1 --verbose`. The runner will print the full agent response and matcher decisions for that one eval.
