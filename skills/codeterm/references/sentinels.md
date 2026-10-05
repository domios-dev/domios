# Sentinels for recurring actions

Use `codeterm sentinel create --help` and `codeterm docs SENTINELS` to author a
bounded graph instead of polling an agent. Sentinels inherit the creator's cwd,
workspace, and Team; Team close removes them.

Build one before you repeat a check or wait on an external, agent or cron
status. "When to use a Sentinel graph" in `codeterm docs SENTINELS` has six
recipes: CI/PR pass-fail routing, a Worker stopped on a prompt, cron run
failures, deploy health, long build/test completion, and a self-improving loop
that escalates `other` to the owning agent for a versioned update.

```bash
codeterm sentinel create --probe "gh run view 456 --json status,conclusion" --question "waiting until CI completes, then finished regardless of conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h
codeterm sentinel create --observe tab:AGENT_TAB --question "Which prompt state is present?" --buckets ready,other --every 10s --timeout 1h
codeterm sentinel list --json
codeterm sentinel show SENTINEL_ID --json
codeterm sentinel stop SENTINEL_ID --json
```

Create with exactly one source: `--probe` or `--observe tab:<id>` /
`--observe cron-run:<id-or-name>`. A CronRun stage binds to the exact tab
verified by the cron store. Replacement resets the observation hash, accepted
bucket and held state, but not the graph's transition count; finish or tab
removal unbinds it. Use `--every` with its 10-second minimum; unchanged output
never calls the Supporter. Without a Choice adapter, use `--match bucket=regex`.

Inline `--route bucket=action` supports `notify`, `sentinel:<id>`, `send:<tab>`,
`cron:<name>` and `stop`. Use `--handlers-file` for command and prompt-response
handlers, explicit handler IDs, and escalation handlers in the reserved `other`
bucket. Only accepted transitions route. Probe output and model verdicts never
become arbitrary commands or targets.

A `respond_prompt:<kind>` handler requires an observed tab or CronRun. It only
requests an answer: the existing safe-response path revalidates the live prompt
and kind, then chooses a provider-declared option. This includes supported
per-tool permission prompts. An unsupported kind, stale prompt or unsafe target
is refused. Sentinel supplies no selected option, provider steps or raw PTY
keys. Each tab accepts at most one response per prompt occurrence; Sentinel
prompt-response requests are limited to 600 per rolling 60-second window.

Reserve `other` handlers for `notify` and `escalate:creator` /
`escalate:agent:<tab>`. An escalation holds the stage until resolution, a new
run binding, or its timeout; its claim is never replayed. A cause can escalate
at most once per five minutes per Sentinel. Resolve it with
`codeterm sentinel escalation resolve SENTINEL_ID TRANSITION_ID`.

Every action stays within the owning agent's authority and creator, Team,
workspace, and machine scope. Graphs default to 64 transitions and cap the
lifetime transition count at 256; a run binding does not reset it. Exhaustion
stops with `transition_budget_exhausted`. Action attempts default to 256 and
`--max-actions` caps them at 1024.

Any caller in the same Team and workspace may amend a graph. Use the current
version and retain lineage:

```bash
codeterm sentinel update SENTINEL_ID --expect-version VERSION --from-file definition.json
codeterm sentinel update SENTINEL_ID --expect-version VERSION --from-file definition.json --cause TRANSITION_ID
codeterm sentinel history SENTINEL_ID
```

<!-- revision: 3 -->
