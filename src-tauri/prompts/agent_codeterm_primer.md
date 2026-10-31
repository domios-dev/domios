## Domios control

Use `domios` for tabs, agents, plans, mesh, memory and schedules. First search
`domios mem search "<need>"`, then read `--help` or `domios docs`; never guess.
Load the `domios` skill for these operations.

A tab title is one word and stays stable. Dispatch derives the description from
the task name (`--description` overrides); direct `agent spawn` needs
`--description`; `domios tab describe` updates it. Finish related edits,
then verify once: skip any check the next step already performs (a rebuild
compiles, a live run tests). In Git, commit in large batches: one commit per
finished task or integration wave, plus one per review round; never per edit,
file or small fix.

Notify through `domios notify send "title" [--from-pad PATH] [--severity info|warning|error] --level verbose|action_needed|failure`; never use provider-native notifications. `verbose` is occasional progress or completion, never every step. `action_needed` (waiting on the user) and `failure` (it broke and you stopped) are rare escalations, only when you need user input and cannot proceed or the user asked; Workers and Planners use their reporting channel instead. `unseen` means the record was kept with exit 0; do not retry.

An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. The owner's next chat message in that tab also resolves its open questions. After receiving an answer elsewhere, close your own question with `domios notify resolve <id>`; resolved questions resume eligible Curator rounds. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.

Blueprints: `blueprint new --intent "<purpose>" --json` first; typed call when a card fits, otherwise author/check/apply. Read `domios docs BLUEPRINT-AUTHORING`; retain the sealed check definition for calls. Re-applying a name retargets it.
