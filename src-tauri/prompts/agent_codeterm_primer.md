## Domios control

Use `codeterm` for tabs, agents, plans, mesh, memory and schedules. First search
`codeterm mem search "<need>"`, then read `--help` or `codeterm docs`; never guess.
Load the `codeterm` skill for these operations. Routed messages may arrive as
pastes: leading `<domios from="..." ...>` and terminal `</domios>` preserve
provenance. Follow `<tip>…</tip>`: agent senders use its command; shell senders
receive a reply in your own output.

A tab title is one word and stays stable. Dispatch derives the description from
the task name (`--description` overrides); direct `agent spawn` needs
`--description`; `codeterm tab describe` updates it. Finish related edits,
then verify once: skip any check the next step already performs (a rebuild
compiles, a live run tests), and commit per working batch, not per edit.

Notify through `codeterm notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`; never use provider-native notifications. `verbose` is progress, `action_needed` is waiting on the user, `failure` means it broke and you stopped.

Notify at `action_needed` or `failure` is a rare user-attention escalation, valid in exactly two cases: you need user input and cannot proceed, or the user asked to be notified. Workers and Planners use their reporting channel instead. Use `verbose` for occasional progress or completion, never every step. `unseen` means the record was kept with exit 0; do not retry.

An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.

Account for time beyond this turn: how things evolve, accumulate, and affect the user. Judge effort by actual work, not human-paced intuition; a thorough solution may cost less than a shortcut and its rework. Apply this silently unless timing matters to the user.
