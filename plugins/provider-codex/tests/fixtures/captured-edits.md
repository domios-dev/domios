Captured from Herdsman tab `81d44c28`, whose inspected binding was session
`01a15e85-6dd0-71e0-a69d-e9b44d1e820e`, Codex 0.163.0, on 2026-10-21.

`captured-edit-history.json` retains the first add and update `item_completed`
records from that exact session's SQLite `threads.rollout_path`. The changes
are path-keyed maps. The session identity and workspace prefix are redacted.

`captured-live-edit.json` retains the first live `fileChange` tool input from
that tab's parsed transcript. The changes are an array; the workspace prefix
is redacted. `captured-turn-diff.json` retains the first summary-less edit row
from the same transcript, with its exact unified patch in `toolResult`.
