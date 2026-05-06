# Map-vs-rg quality bench (v2 — internal Task tool variant)

You are the bench orchestrator running inside Claude Code. Your job: measure whether the codeterm `map` subsystem actually helps answer concrete questions about this codebase, vs the same agent without `map`.

## Inputs

- Repo: `/Users/rollacode/Developer/codeterm` (you are running here)
- Puzzle pack: `bench/puzzles/puzzles.json` (13 puzzles, each with verifiable ground truth in `expected.must_contain`)

## How to spawn workers

**Use Claude Code's built-in `Agent` tool** (NOT `codeterm agent spawn`). This keeps everything inside one process — simpler, no cross-pane orchestration, no socket games.

For each of the 13 puzzles, fire TWO Agent tool calls in parallel — one with `subagent_type: "general-purpose"` and a `prompt` that includes the **map** preamble, one with the **nomap** preamble. Per the Claude Code docs, sending multiple Agent tool uses in a single response runs them concurrently — do that.

### Mode preambles (inject verbatim into each sub-agent's prompt)

**map mode prompt:**
> You are answering ONE question about the codeterm repo at /Users/rollacode/Developer/codeterm. Prefer `codeterm map` (search / rank / bridges / edges / deps / dependents / files) for symbol/concept/centrality/bridge/dependency lookups. Use `rg` only for literal escape codes, raw text in comments/strings, or anonymous closures. The codeterm rule at ~/.claude/rules/codeterm.md describes the full map command surface.
>
> QUESTION: {question}
>
> Provide your final answer as a single line in your response, prefixed `ANSWER:`. Then on the next line, write `TOOL_USES: N` where N is the integer count of tool calls you made. Do not write anything after that.

**nomap mode prompt:**
> You are answering ONE question about the codeterm repo at /Users/rollacode/Developer/codeterm. Do NOT use `codeterm map` at all — pretend that subsystem does not exist. Use only Read, rg/grep, find, head/tail, sed, awk.
>
> QUESTION: {question}
>
> Provide your final answer as a single line in your response, prefixed `ANSWER:`. Then on the next line, write `TOOL_USES: N` where N is the integer count of tool calls you made. Do not write anything after that.

## Grading

For each (puzzle, mode) pair, the Agent tool call returns a string. Parse `ANSWER:` and `TOOL_USES:` lines.

Then check `expected.must_contain` from puzzles.json against the answer text:

- For each required token, do case-insensitive substring match.
- For numeric tokens listed in `line_alt`, accept ANY of `[must_contain[i], line_alt[0], line_alt[1], ...]` for that position.
- All required tokens hit → `correctness = 1.0`
- ≥ 50% hit → `correctness = 0.5`
- < 50% → `correctness = 0.0`

## Output

Print a markdown table:

| # | Puzzle | Kind | Expected strength | Map ✓ | Nomap ✓ | Map tools | Nomap tools |

Then aggregate rows:

- Correctness: map vs nomap (sum/13)
- Avg tool_uses: map vs nomap
- Wins by category (map / nomap / tie)

Group puzzles by `kind` in a small breakdown table (e.g. "centrality 3/3 map wins" etc).

End with a 3-5 sentence verdict: where map clearly wins, where it is irrelevant, where rg dominates, what surprised you.

## Notes

- Spawn all 26 sub-agents in parallel (one Agent tool call per (puzzle, mode)). Claude Code supports multiple parallel Agent calls in a single response.
- If a sub-agent times out or errors, treat as `correctness=0, tool_uses=null`.
- Keep your own response brief — the meat is the table + verdict.
- Do not call `codeterm agent spawn` or open new panes. Everything stays inside this one Claude Code process via Agent tool.
