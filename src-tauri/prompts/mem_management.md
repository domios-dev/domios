## Memory (`codeterm mem`)
The memory protocol is in your provider rules; on top of it:
- Save only durable facts. Instructions for the current session or task (model or worker choice for now, temporary priorities, 'this time', 'for this release') stay in the conversation or plan, never in memory. If durability is uncertain, do not save; ask only when the user says remember. Even an explicit remember of a session-scoped instruction requires the user to say it is permanent.
- "Remember X" (durable facts): search first (`mem search --json`), then save at once in English (`mem save --json --kind fact`), one command at a time, using the ids it returns.
- Project-specific facts: tag them with `--area <project>`; facts that only matter inside one workspace also get `--workspace <id>`, so wake-up shows them there. Find them again with `mem search --area <project>`.
- A fact with a validity window (joins/leaves, starts/ends) also gets a graph edge: `codeterm mem graph relate --from ID --rel RELATION --to ID --valid-from DATE [--valid-to DATE]`, with real returned ids only.
