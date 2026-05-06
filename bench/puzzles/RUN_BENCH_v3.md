# Bench v3 — multi-step tasks with trajectory metrics

You are the bench orchestrator running inside Claude Code. The pack at `bench/puzzles/puzzles_v3.json` has 6 multi-step puzzles. Each task should require multiple steps (locate → trace → aggregate or locate → confirm → list). The point is to measure whether `codeterm map` saves the agent a chain of useless rg-calls when the question is structural.

## Inputs

- Repo: `/Users/rollacode/Developer/codeterm`
- Pack: `bench/puzzles/puzzles_v3.json` (6 tasks, each with `expected.must_contain`)
- Two modes per task: `map` and `nomap`
- N=3 runs per (task, mode) → 36 sub-agents total

## Spawning sub-agents

Use Claude Code's built-in `Agent` tool, `subagent_type: "general-purpose"`. Issue all 36 calls in ONE assistant message — they run in parallel.

### Mode preambles (verbatim)

**map mode prompt skeleton:**
> You are answering ONE multi-step question about the codeterm repo at /Users/rollacode/Developer/codeterm. The question may require chaining 2-3 lookups (e.g. find centrality → confirm caller → list).
>
> Prefer `codeterm map` (search / rank / bridges / edges / deps / dependents / files) for symbol/concept/centrality/bridge/dependency lookups. Use `rg` only for literal escape codes, raw text inside string literals/comments/docs, anonymous closures, or post-processing structured map output. The codeterm rule at ~/.claude/rules/codeterm.md describes the full map command surface.
>
> QUESTION:
> {question}
>
> Provide your final response in this exact 3-line format:
> ```
> ANSWER: <single-line answer with all required tokens>
> TOOL_USES: <integer count of tool calls you made>
> TOOL_BREAKDOWN: <comma-separated list of tool TYPES used, e.g. "codeterm-map,rg,Read">
> ```

**nomap mode prompt skeleton:**
> You are answering ONE multi-step question about the codeterm repo at /Users/rollacode/Developer/codeterm. Do NOT use `codeterm map` at all — pretend that subsystem does not exist. Use only Read, rg/grep, find, head/tail, sed, awk, sqlite3 (no, not sqlite either — pretend the index sqlite doesn't exist).
>
> QUESTION:
> {question}
>
> Provide your final response in this exact 3-line format:
> ```
> ANSWER: <single-line answer with all required tokens>
> TOOL_USES: <integer count of tool calls you made>
> TOOL_BREAKDOWN: <comma-separated list of tool TYPES used, e.g. "rg,Read,Bash">
> ```

## Grading

For each (task, mode, run) result, parse the 3-line block. Then:

- For each `must_contain` token in the puzzle, do case-insensitive substring match in the answer line.
- If 100% present → correctness=1.0. If ≥50% present → 0.5. Else 0.0.

For each (task, mode) compute the **MEDIAN** correctness of the 3 runs (with N=3, median = the middle value of [run1, run2, run3]).

Track aggregates:
- Total correctness sum across 6 tasks per mode
- Avg tool_uses per mode
- Tool-mix breakdown: how often did map-mode actually USE a `codeterm map` command vs default to rg?

## Output

Print this report:

```
| # | Task | Kind | Strength | map_score | nomap_score | map_tools | nomap_tools | map_tools_used |
|---|---|---|---|---|---|---|---|---|

Aggregates:
- Correctness: map X.X/6 vs nomap Y.Y/6
- Avg tool_uses: map A vs nomap B (delta %)
- Map-mode actually-used-codeterm-map: K/6 tasks (%)

Stratified by `expected_strength`:
- "map" strength tasks: map W/4 vs nomap V/4
- "rg" strength tasks: map U/2 vs nomap T/2

Variance:
- For each task with both runs differing → list which (these are noisy and should be re-run later).
```

Then 4-6 sentence verdict covering:
1. Where map clearly differentiates (cite tasks)
2. Where adversarial literal tasks confirmed rg's natural domain (or where map surprisingly held up)
3. Trajectory metric story: did map-mode actually invoke `codeterm map` or did the agent default to rg even when permitted?
4. What the variance bands tell us about whether ~6 tasks is enough for confident claims
5. One concrete improvement recommendation for the v4 bench or for the codeterm rule

## Notes

- ALL 36 spawn calls in ONE message. Wait for all to return as a batch.
- If a sub-agent times out, treat as score=0, tool_uses=null, breakdown="(timeout)".
- Keep your prose tight, data-dense. Tables before paragraphs.
- Sub-agents inherit cwd = /Users/rollacode/Developer/codeterm.
