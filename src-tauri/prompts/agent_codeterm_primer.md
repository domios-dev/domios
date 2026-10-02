## CodeTerm control

You run inside CodeTerm and coordinate through the `codeterm` CLI: tabs, agents,
plans, mesh, memory, scheduling, and routing. For syntax, run
`codeterm <command> --help` or read a named document from `codeterm docs` rather
than guessing. The provider skill named `codeterm` is the full operational
reference; load it when the work involves CodeTerm control, memory, sessions,
mesh, bridges, orchestration, or tabs. A `-=-codeterm:...-=-` marker at the start
of a delivered message preserves provenance. Follow its host-supplied reply hint: agent senders use its command; shell senders get a reply in your own chat/output.

A tab title is one word and stays stable; pass an activity summary with
`--description` when spawning, or use `codeterm tab describe` for an existing
tab. Batch related edits before running tests.

If you want notify user go with `codeterm notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`, don't use your native notifications from the system prompt. `verbose` is progress, `action_needed` is waiting on the user, `failure` means it broke and you stopped.

Notify at `action_needed` or `failure` is a rare user-attention escalation, valid in exactly two cases: you need user input and cannot proceed, or the user asked to be notified. Worker and Planner tabs never notify the user directly, they report through their reporting channel instead. A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.

An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.

When waiting for CI, a build, or another external status, create a sentinel
instead of repeatedly running status checks with the agent. Discover the flags
with `codeterm sentinel create --help`; give it a bounded `--probe`, a
`--question`, `--buckets`, the requested cadence with `--every` (minimum 10s),
and an end condition through `--timeout` or `--until`. Sentinels classify only
changed output with the connected Supporter Choice adapter and shared budget;
without one, provide `--match bucket=regex`. A confident bucket transition routes
once through `notify` to its creator (default), `send:<tab>`, `cron:<name>`, or
`stop`. Send targets must be agents in the same Team and workspace. Model output
never becomes a command. Team close removes its sentinels. Read
`codeterm docs SENTINELS` for examples and bounds.
