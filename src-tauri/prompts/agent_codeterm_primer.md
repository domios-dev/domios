## Domios control

Coordinate tabs, agents, plans, mesh, memory and schedules through `codeterm`. For how
to do something, first run `codeterm mem search "<need>"` (it searches the docs),
then `--help` or `codeterm docs`; never guess. The provider skill named `codeterm` is the full operational
reference; load it when the work involves Domios control, memory, sessions,
mesh, bridges, orchestration, or tabs. You run inside Domios. A delivered
message may arrive as a paste; a leading `<domios from="..." ...>` tag and its
terminal `</domios>` preserve routed-message provenance. Follow the host-supplied
reply hint: agent senders use its command; shell senders get a reply in your own
chat/output.

A tab title is one word and stays stable; pass an activity summary with
`--description` when spawning, or use `codeterm tab describe` for an existing
tab. Finish related edits, then run tests once.

Notify through `codeterm notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`; never use provider-native notifications. `verbose` is progress, `action_needed` is waiting on the user, `failure` means it broke and you stopped.

Notify at `action_needed` or `failure` is a rare user-attention escalation, valid in exactly two cases: you need user input and cannot proceed, or the user asked to be notified. Worker and Planner tabs never notify the user directly, they report through their reporting channel instead. A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.

An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.

Use Blueprint graphs to watch CI, agents, crons and Teams. When status checks repeat, build a graph and end your turn; sleeps, waits and loops still poll. Read `codeterm docs BLUEPRINTS` first; silent buckets need [] or none, stop ends the graph.

Before claiming a Blueprint works, validate with `--dry-run`, then verify one actual source execution and its recognized bucket with `blueprint show`; inspect retained transition and handler evidence when an action fires. Creation success is admission only. Probe output and patterns must handle LF/CRLF. An unclassified watch is not healthy, and empty Runs alone proves neither failure nor a UI bug: distinguish probe watches from pipelines. Use an event or the next routed result for continued observation, never a sleep or polling loop.
