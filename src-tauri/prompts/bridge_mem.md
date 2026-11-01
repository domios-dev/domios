# Domios Memory Protocol

`domios mem` is persistent vector + keyword storage; every query fuses FTS5 and embeddings.

1. Write titles, bodies, summaries and queries in English; translate before saving and reply to the user in their language.
2. Before answering about a person, project or past event, search `mem search` instead of relying on vague recollection. If unsure of a name, date, decision or config, say you will check, then search.
3. For identity questions (you, the user, this machine), run `domios mem wake-up` for the L0 identity and L1 critical facts.
4. Use only IDs returned by search; never guess an ID for `domios mem get`.
5. Save durable facts only. Keep session/task instructions (current model or worker choice, temporary priorities, “this time,” “for this release”) in the conversation or plan. If durability is unclear, do not save; ask only when the user says “remember.” Even then, save a session-scoped instruction only after the user says it is permanent. When the user says “remember” for a durable fact, in any language, save it now in English with `domios mem save`; a background worker handles area, entities, graph links and dedup.
6. When facts change, save the new fact; history keeps the old. Archive outdated useful facts; use `mem delete` only for explicitly permanent removal.
7. Leave dates out of bodies (`created_at`/`updated_at` are tracked); duplicates pollute vector search.
8. Before calling a skill missing, run `domios mem search "<capability> skill"` (`domios mem skill list` if needed); read the returned `SKILL.md` before acting.

## Where memory lives

- Durable facts live only in `mem`; provider auto-memory and side stores such as `~/.claude/projects/*/memory/` vanish when switching providers.
- Domios regenerates the `CLAUDE.md`/`AGENTS.md`/`AGENT.md` copies it writes into a project root on every start, so edits are lost. A tracked repo `AGENTS.md` is ordinary source.
- Register skills with `domios mem skill new <slug>` (folder) or `domios mem save --kind skill ...` (single body), never by writing to `~/.claude/skills` or `~/.agents/skills`.
- Specific saves are searchable; “did some work” is noise. Past conversations are in `domios sessions search`; durable facts are in `domios mem search`.

## Where to find more

- Provider skill `domios` has the full command catalog for kinds, filters, graph queries, search fallbacks, pin/learn, calibration, keepawake, dreaming, sessions and skill folders. The short “codeterm skill” mem item is only a stub.

<!-- revision: 18 -->
