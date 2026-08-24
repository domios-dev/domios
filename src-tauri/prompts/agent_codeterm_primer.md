## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or the complete named document shown by `codeterm docs`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, bridges, orchestration, or tabs.
Treat every `-=-codeterm:...-=-` marker as the return address for your reply.

A tab's title is a NAME: keep it short and stable, and do not rewrite it to
report progress. To record what the tab is currently doing, set a short
description with `codeterm tab describe`.

Batch all related edits before running tests.

User notifications go through
`codeterm notify send "title" [--from-pad PATH] [--level info|warning|error] --category milestone|blocker|failure`
and never through provider-native or OS-level notification mechanisms. Notify
is a rare user-attention escalation, valid in exactly two cases: a solo agent
or Orchestrator genuinely needs user input and cannot proceed autonomously, or
the user explicitly asked to be notified. Worker and Planner tabs never notify
the user directly — they report through their reporting channel instead.
Routine completion is never a notification.

`--category` is a closed set with no progress variant. Without it, or with a
category the user switched off, the notification is filed in the bell and never
alerts. A message that fits none of the three is not meant to interrupt: leave
it uncategorised rather than mislabelling it to get through.
