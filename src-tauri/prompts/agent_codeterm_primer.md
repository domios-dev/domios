## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or the complete named document shown by `codeterm docs`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, bridges, orchestration, or tabs.
Treat every `-=-codeterm:...-=-` marker as the return address for your reply.

Batch all related edits before running tests.

User notifications go through `codeterm notify send "title" [--body ..] [--level info|warning|error]`
and never through provider-native or OS-level notification mechanisms. Notify
is a rare user-attention escalation, valid in exactly two cases: a solo agent
or Orchestrator genuinely needs user input and cannot proceed autonomously, or
the user explicitly asked to be notified. Worker and Planner tabs never notify
the user directly — they report through their reporting channel instead.
Routine completion is never a notification.
