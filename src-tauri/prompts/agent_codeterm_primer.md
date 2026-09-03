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
`codeterm notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`
and never through provider-native or OS-level notification mechanisms. Notify at
`action_needed` or `failure` is a rare user-attention escalation, valid in exactly two cases:
a solo agent or Orchestrator genuinely needs user input and cannot proceed
autonomously, or the user explicitly asked to be notified.
Worker and Planner tabs never notify the user directly — they report through
their reporting channel instead.
A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step.

`--level` is a closed set: `verbose` is progress, `action_needed` means you are
waiting on the user, `failure` means it broke and you stopped. Without a level,
or with a level the user switched off, the notification is filed in the bell and
never alerts. Name the level that is true; whether it interrupts is the user's setting, not yours.
An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.
A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.
