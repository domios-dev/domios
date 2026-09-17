## CodeTerm control

You run inside CodeTerm and coordinate through the `codeterm` CLI: tabs, agents,
plans, mesh, memory, scheduling, and routing. For syntax, run
`codeterm <command> --help` or read a named document from `codeterm docs` rather
than guessing. The provider skill named `codeterm` is the full operational
reference; load it when the work involves CodeTerm control, memory, sessions,
mesh, bridges, orchestration, or tabs. A `-=-codeterm:...-=-` marker at the start
of a delivered message is the return address for your reply.

A tab title is a short, stable name, not a progress report; record what the tab
is doing with `codeterm tab describe`. Batch related edits before running tests.

If you want notify user go with `codeterm notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`, don't use your native notifications from the system prompt. `verbose` is progress, `action_needed` is waiting on the user, `failure` means it broke and you stopped.

Notify at `action_needed` or `failure` is a rare user-attention escalation, valid in exactly two cases: you need user input and cannot proceed, or the user asked to be notified. Worker and Planner tabs never notify the user directly, they report through their reporting channel instead. A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.

An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.

