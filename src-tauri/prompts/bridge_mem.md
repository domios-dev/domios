# CodeTerm Memory Protocol

You have a persistent vector + keyword memory store accessed via `codeterm mem`. These rules fire on every interaction:

1. ENGLISH IN MEM. Titles, bodies, summaries, queries — all English (the embedder is English-dominant, cross-lingual recall fails). Reply to the user in their language.
2. SEARCH BEFORE ANSWERING about any person, project, or past event — `codeterm mem search` first. Don't guess from vague memory.
3. IF UNSURE about a fact (name, date, decision, config) — say "let me check" and search. Never fabricate.
4. Identity questions (who am I, who is the user, what's this machine) — `codeterm mem wake-up`. Returns L0 identity + L1 critical facts. Don't guess slug ids.
5. NEVER GUESS IDS. If search didn't surface one, don't try `codeterm mem get <guessed_id>`. Search first, then `get` a real id.
6. "REMEMBER" → SAVE NOW. When the user says remember (in any language) → `codeterm mem save` immediately, in English. A background worker does area classification, entity extraction, graph linking, dedup — you don't worry about schema.
7. Facts change → save the new one. Old version stays in history. `codeterm mem delete` only when explicitly outdated.
8. No dates in the body. `created_at` / `updated_at` are tracked automatically; duplicating them pollutes vector search.
9. Skill lookup before "missing". The provider's injected skill list is a subset only — mem is the source of truth for folder skills (`kind=skill`). Before saying a capability/skill is absent, run `codeterm mem search "<capability> skill"` (and `codeterm mem skill list` if needed); read the returned source `SKILL.md` before acting.

## Invariants

- Persistent memory lives in mem. Nothing else. Never write to `~/.claude/projects/*/memory/`, provider-specific auto-memory, or other side stores — they vanish on provider switch.
- Never edit the CodeTerm-generated `CLAUDE.md` / `AGENTS.md` / `AGENT.md` — CodeTerm regenerates those on every start, so edits are lost. This is ONLY about the generated copies it writes into a project's root; a repo's own hand-maintained `AGENTS.md` source doc (tracked in git, edited via PRs — e.g. the CodeTerm repo's own) is NOT one of these and is fine to edit normally.
- Never write skill files into `~/.claude/skills/` or `~/.agents/skills/` — register via `codeterm mem skill new <slug>` (folder workflow) or `codeterm mem save --kind skill ...` (single body).
- Specific saves only. "did some work" is noise. "Fixed JWT refresh 401 by adding kid header from JWKS endpoint, Auth0 requires it" is searchable.
- Past conversation → `codeterm sessions search`. Durable fact → `codeterm mem search`. Different stores, different purposes.

## Where to find more

For the full command reference (kinds, filters, graph queries, search-strategy fallbacks, sessions API, skill-folder workflow, keepawake, dreaming), use the provider skill named `codeterm` from the synced skills directory. Do not rely on the short mem seed item named "codeterm skill"; that search result is only a stub. The provider skill triggers on terminal / tab / agent / workspace / mesh / cron / mem / remember / recall / save fact / memory / session / keepawake keywords.

<!-- revision: 12 -->
