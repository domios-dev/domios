# CodeTerm Memory Protocol

`codeterm mem` is your persistent vector and keyword memory, and it is how facts outlive a session or a provider:

1. Write memory in English (titles, bodies, summaries, queries), because the embedder is English-dominant and cross-lingual recall fails. Reply to the user in their language.
2. Before answering about a person, project, or past event, run `codeterm mem search` instead of trusting a vague recollection. When unsure of a name, date, decision, or config, say you will check, then search.
3. For identity questions (who am I, who is the user, what is this machine), `codeterm mem wake-up` returns the L0 identity and L1 critical facts.
4. Use ids a search returned; `codeterm mem get` on a guessed id finds nothing useful.
5. When the user says remember (in any language), save it now with `codeterm mem save`, in English. A background worker handles area, entities, graph links, and dedup.
6. When a fact changes, save the new one; history keeps the old. Archive outdated facts that may still be useful; use `codeterm mem delete` only for an explicitly permanent removal.
7. Leave dates out of bodies: `created_at` and `updated_at` are tracked, and duplicates pollute vector search.
8. Before calling a skill missing, run `codeterm mem search "<capability> skill"` (and `codeterm mem skill list` if needed), since the provider's injected skill list is only a subset; read the returned source `SKILL.md` before acting.

## Where memory lives

- Durable memory lives only in mem. Provider auto-memory and side stores such as `~/.claude/projects/*/memory/` vanish on a provider switch.
- CodeTerm regenerates the `CLAUDE.md` / `AGENTS.md` / `AGENT.md` copies it writes into a project root on every start, so edits there are lost. A repository's own tracked `AGENTS.md` is an ordinary source file.
- Skills are registered with `codeterm mem skill new <slug>` (folder workflow) or `codeterm mem save --kind skill ...` (single body), not written into `~/.claude/skills/` or `~/.agents/skills/`.
- Specific saves are searchable ("Fixed JWT refresh 401 by adding kid header from JWKS endpoint, Auth0 requires it"); "did some work" is noise.
- Past conversations are in `codeterm sessions search`; durable facts are in `codeterm mem search`.

## Where to find more

The provider skill named `codeterm` from the synced skills directory is the full command reference (kinds, filters, graph queries, search-strategy fallbacks, sessions API, skill-folder workflow, keepawake, dreaming). The short mem seed item named "codeterm skill" is only a stub.

<!-- revision: 13 -->
