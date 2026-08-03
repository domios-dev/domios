# Prompt architecture

How CodeTerm composes the prompts that drive the General Agent, project workers, and headless mem workers. This document explains where each piece lives, what gets included where, and how to decide which file a new instruction belongs in.

## Layout

```
src-tauri/prompts/
  general_agent.md               GA prompt — GA-unique content only; includes {codeterm_cli}
  codeterm_cli.md                Shared CLI reference — role-neutral, single source of truth
  bridge_mem.md                  Mem protocol installed into each provider's rules dir
  codeterm_rule.md               Bridge rule appended to AGENTS.md, teaches CLI to spawned agents
  bootstrap_instructions.md      First-run bootstrap.md template
  worker_suffix.md               Appended to Worker task bodies
  orchestrator_suffix.md         Appended to Orchestrator task bodies
  cron_complete_suffix.md        Appended to every cron task
  cron_report_hint.md            Appended to cron tasks when creator_tab_id is set
  worker_task_preamble.md        Wrapper for headless mem worker tasks
  mem_extraction.md              Prompt for auto-extraction (haiku graph worker)
  mem_rerank.md                  Prompt for search-result reranking
  dreaming_rem.md                REM-phase dreaming prompt

skills/codeterm/
  SKILL.md                       Project-agent CLI reference — template with {codeterm_cli}
  references/
    mem.md                       Full mem command catalog
    roles.md                     Agent-role rules + decision tree
    cron.md                      Cron flag reference
    telegram.md                  Bridge setup, formatting, migration
    themes.md                    Theme TOML format

docs/                            Built-in docs (loaded on demand via `codeterm docs <NAME>`)
  CONFIG.md, API.md, MESH.md, MEMORY.md, BUILD.md, PROMPTS.md, SKILLS.md, TESTING.md
  TELEGRAM-SETUP.md              Operational playbook — wiring a new Telegram bot
  SECRETS.md                     Operational playbook — `mem secret` CLI ladder
  TEXT-HOOKS.md                  Operational playbook — inbound message hook daemon
```

## Compile and substitution flow

`src-tauri/src/prompts.rs` declares each prompt file as a `const &str` via `include_str!()` — they're baked into the binary at compile time. Edit the `.md`, run `./build.sh check`, ship.

### GA prompt rendering

`src-tauri/src/service/general_agent/instructions.rs::render_agent_md` substitutes placeholders into `general_agent.md`:

| Placeholder | Source |
|-------------|--------|
| `{version}` | `env!("CARGO_PKG_VERSION")` |
| `{platform}` | `cfg!(target_os = ...)` |
| `{hostname}` | `hostname::get()` |
| `{provider_name}` | runtime arg (`claude` / `codex` / etc.) |
| `{general_dir}` | `~/.codeterm/general/` |
| `{secrets_dir}` | `~/.codeterm/secrets/` |
| `{skills_dir}` | `~/.codeterm/skills/` |
| `{identity}` | `general/identity.md` contents |
| `{soul}` | `general/soul.md` contents wrapped in `## Soul\n...` |
| `{wake_up}` | `mem.wake_up()` L0+L1 snapshot wrapped in `## Live mem snapshot\n...` |
| `{codeterm_cli}` | `prompts/codeterm_cli.md` contents (shared CLI reference) |

The rendered output is written to three files in `general/`:

- `AGENT.md` — the rendered GA prompt (what the agent reads via `@./AGENT.md`)
- `CLAUDE.md` — a thin re-export: `@./AGENT.md`
- `AGENTS.md` — `agent_content + codeterm_rule + bridge_mem` (catch-all for tools that look here)

`bootstrap.md` is created **only on first run**; if it already exists, regen leaves it alone.

### Skill rendering

`skills/codeterm/SKILL.md` is also a template with `{codeterm_cli}`. On daemon startup, `commands::skills::seed_bundled_skills` copies the bundled `skills/codeterm/` directory to `~/.codeterm/skills/codeterm/` (or `~/.codeterm-dev/skills/codeterm/` for dev). After the copy, the seeder reads the destination `SKILL.md`, replaces `{codeterm_cli}` with the same string baked into the binary (`crate::prompts::CODETERM_CLI`), and writes it back.

That keeps the GA prompt and the project-agent skill in lock-step: both pull the CLI surface from one file (`prompts/codeterm_cli.md`).

If you change `prompts/codeterm_cli.md`:
- GA picks it up on next session start (because `instructions.rs::generate_instructions` regenerates AGENT.md).
- Project agents pick it up next time `seed_bundled_skills` runs (on daemon startup) — older copies in data-dir are overwritten when the source revision tag is higher than the destination's.

## Decision matrix — where does a new instruction belong?

When you're about to add or move a piece of prompt content, ask:

| Question | Where it goes |
|----------|---------------|
| Does it teach **CLI syntax** (commands, flags, examples) that's the same for the GA and project agents? | `prompts/codeterm_cli.md` (shared) |
| Is it a **safety rule** the GA must check on every turn (prefix routing, never-kill-prod, never-auto-commit)? | `prompts/general_agent.md` |
| Is it a **decision policy** that's GA-specific (implicit→ask, recon-before-action, mesh→Orchestrator, workspaces-are-commitments)? | `prompts/general_agent.md` |
| Is it a **lifecycle hook** that only the GA bridges (forward report to originating channel, init handshake, voice transcribe awareness)? | `prompts/general_agent.md` |
| Is it an **operational ladder** that fires rarely (Telegram bot setup, secrets store CLI, hook daemon config)? | `docs/<TOPIC>.md` + register in `cli/src/cmd/docs.rs` |
| Is it **project-agent framing** (workspace/Team authority and the no-General-Agent fallback boundary)? | `skills/codeterm/SKILL.md` (the non-template intro and outro around `{codeterm_cli}`) |
| Is it **mem protocol** (English-only, search-first, no fabrication)? | `prompts/bridge_mem.md` (installed into provider rules dirs by `service::bridge_rule`) |
| Is it **a worker-task addendum** (must report, ack receipt, return exit code)? | `prompts/worker_suffix.md` / `orchestrator_suffix.md` |
| Is it **per-cron-task boilerplate** (run id, completion call, report hint)? | `prompts/cron_complete_suffix.md` / `cron_report_hint.md` |
| Is it a **headless worker prompt** (extraction, rerank, dream)? | `prompts/mem_extraction.md` / `mem_rerank.md` / `dreaming_rem.md` |

### Anti-patterns

- **Don't duplicate CLI syntax in both GA prompt and skill.** That's exactly what `{codeterm_cli}` substitution is for. If you find yourself writing `codeterm tab list` in `general_agent.md`, you're in the wrong file.
- **Don't bake operational ladders into the GA prompt.** If a procedure fires once a month (initial bot setup, vault unlock, hook daemon config), it's an on-demand playbook in `docs/`, not a per-turn instruction.
- **Don't add per-turn safety to a playbook.** If a rule must fire on every inbound message (e.g. "trust `[Hook metadata:`, don't re-classify"), it lives in the GA prompt — one line referencing the playbook for the full how-to.
- **Don't write `{placeholder}` in the GA prompt without wiring it in `instructions.rs`.** Missing substitutions ship the literal `{placeholder}` text to the agent.
- **Don't paste the GA prompt into spawned agents' `--task`.** They get `bridge_mem.md` automatically via the per-provider rules path and that teaches `codeterm mem *`. The GA prompt is GA-only.

## Adding a new placeholder

1. Add the substitution in `instructions.rs::render_agent_md` — `.replace("{my_var}", value)`.
2. Reference it in `general_agent.md` where you want the value injected.
3. Add a unit test in `instructions.rs::tests` that asserts the placeholder doesn't survive into the rendered output (`assert!(!rendered.contains("{my_var}"))`).
4. Document the new placeholder in the table above.

## Adding a new on-demand playbook to `codeterm docs`

1. Write the playbook as `docs/<TOPIC>.md`. Use a short, ALL-CAPS slug for the doc name (`TELEGRAM-SETUP`, not `telegram-setup`).
2. Register it in `cli/src/cmd/docs.rs`:
   ```rust
   const DOCS: &[(&str, &str)] = &[
       // ...
       ("TOPIC", include_str!("../../../docs/TOPIC.md")),
   ];
   ```
3. Mention it in `general_agent.md` under the **Operational playbooks** section so the GA knows it exists. One line — name + when to use it.
4. Bump `<!-- revision: N -->` at the end of the playbook on every meaningful edit.

## Adding a new mem worker prompt

Mem workers (extraction, rerank, dream) are headless — they don't share context with the GA. Their prompts go in `prompts/<name>.md` and are loaded via `include_str!` in `prompts.rs`. Use `{placeholder}` style for inputs the worker code substitutes (e.g. `{query}`, `{items}`, `{intent}`, `{worker_id}`, `{task}`). Document the placeholders at the top of the prompt file as a comment.

## Revision tags

Files that ship as content (skills, docs, bundled prompts) carry a `<!-- revision: N -->` line at the bottom. Bump it on every change. The skill seeder uses revision tags to decide whether to overwrite a stale copy in data-dir.

GA prompt and codeterm_cli.md don't carry revision tags — they're not user-facing and get rebuilt on every binary release.

<!-- revision: 3 -->
