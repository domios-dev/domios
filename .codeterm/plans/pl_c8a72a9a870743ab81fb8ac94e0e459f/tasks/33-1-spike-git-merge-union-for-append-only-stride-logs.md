First bounded §33 storage slice. Do not implement the whole explode/append-log/compaction rewrite.

Objective:
- Verify git's built-in merge=union behavior for EOF-appended JSONL stride/event logs, as required by §33.5 before choosing jsonl over one-file-per-event.
- Add the minimal repo rule needed for future append-only plan logs, likely `.codeterm/plans/**/strides.jsonl merge=union` or a similarly scoped attribute.
- Add an automated regression test or script that creates a temporary git repo, performs concurrent branch appends to a jsonl file, merges, and proves both events survive with deterministic fold/dedup expectations.
- Document the exact conclusion near the test or in a short §33 note if needed.

Acceptance:
- The test fails if union merge drops either branch's appended line.
- The test is deterministic and does not touch the user's real repo state outside a tempdir.
- No broad storage migration, no schema rewrite, no CRDT/custom merge driver.
