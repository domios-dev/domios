# DeepSeek Prompt Gap Analysis — 2026-05-25

My personal pass over every prompt, every doc cross-reference, every CLI claim
in the CodeTerm prompt surface. I went file-by-file, command-by-command, line by
line. What follows is what I found — unfiltered, no priority buckets, just the
signal.

---

## 1. lead_suffix.md is a ghost

The file doesn't exist. The const `LEAD_SUFFIX` doesn't exist. The role handler
in both `service/spawn.rs` and `service/orchestrator.rs` only matches `Worker`
and `Orchestrator` — Lead falls through to the default (no suffix).

Yet `docs/PROMPTS.md` has it in the table. `docs/contributing/prompts.md` lists
it as a valid option. The prompt-evaluator skill references it. The
`skills/codeterm/SKILL.md` frontmatter mentions `Orchestrator/Lead/Worker`.

The codebase deliberately flattened to 2 levels (`AgentRole` enum has exactly
two variants). The Leads died but their tombstones stayed in the prompts.

**Impact:** If an agent is spawned with `--role Lead`, it gets nothing appended.
No suffix, no guidance, no reporting instruction. It's alive but untold what
to do.

---

## 2. `codeterm mem worker spawn` — advertised, not implemented

`seed/skill_codeterm.md`:
```
codeterm mem worker spawn --intent "enrich" --task "..."
```

I searched the entire CLI source. There is no handler for this. The word
"spawn" paired with "worker" only appears in comments about PTY internals and
in the dreaming module's `spawn_rem_worker()` — which is internal API, not a
CLI command.

The prompt tells agents they can do something the system can't execute. An
agent that follows this will get a "command not found" or similar error, and
may silently retry or give up.

---

## 3. The GA spec document is a time capsule from a different project

`docs/GENERAL_AGENT.md` (365 lines, revision 2) describes a system that was
planned and partially spec'd but never built the way it's written. Let me be
specific about what's dead:

**Marker-based file management:**
The doc describes `<!-- CODETERM-AUTO-START -->` / `<!-- CODETERM-AUTO-END -->`
markers in AGENT.md that preserve user edits between auto-regenerations.
The actual `instructions.rs` generates vanilla AGENT.md with no markers.
The test at `instructions.rs:184-187` explicitly asserts:
```rust
assert!(!template.contains("CODETERM-AUTO-START"));
```

This marker mechanism was either abandoned or never started.

**EXTRA_INSTRUCTIONS.md:**
The spec says this file is "Created empty on first run, Never touched by
CodeTerm, Agent told to read it." The code never creates it, never references
it. Another dead feature.

**{connected_services} placeholder:**
The spec shows this placeholder in the template. The actual
`general_agent.md` doesn't have it. The actual `instructions.rs` doesn't
substitute it.

**Secrets as flat files:**
Spec says secrets are `~/.codeterm/secrets/*.md` files with human-readable
markdown. The actual system uses `codeterm mem secret` — encrypted store
backed by OS keychain or Bitwarden, never plaintext on disk. The spec
describes a system that was replaced before GA launch.

**{codeterm_skill_content}:**
Spec uses `{codeterm_skill_content}` in its template example. The actual
template uses `{codeterm_cli}` — different name, different content.

**Phase 0-6 architecture:**
The doc describes six implementation phases: Backend PTY autonomy, Protected
workspace, GA foundation, Startup+recovery, Messaging bridge, OS autostart,
Self-management. This reads like a roadmap document that was never updated to
say which phases shipped and which didn't. Discord integration, webhook
receiver, OS autostart, heartbeat cron — none of these exist in the codebase
except as config schema stubs.

**This is not a doc problem — this is a trust problem.** Anyone reading this
doc will form a wrong mental model of the GA. The doc has revision 2 and no
one has touched it since early design.

---

## 4. `docs/PROMPTS.md` table of contents has decayed

The central reference table at lines 30-43 claims file locations and injection
points that are wrong. I checked every row:

| Row | Claimed path | Actual path |
|-----|-------------|-------------|
| general_agent.md | `service/general_agent.rs:114` | `service/general_agent/instructions.rs:91` |
| bootstrap_instructions.md | `service/general_agent.rs:151` | `service/general_agent/instructions.rs:157` |
| worker_suffix.md | `service/orchestrator.rs` only | ALSO `service/spawn.rs:312` |
| orchestrator_suffix.md | `service/orchestrator.rs` only | ALSO `service/spawn.rs:313-318` |
| lead_suffix.md | exists | **does not exist** |
| cron_complete_suffix | `service/cron.rs:139` | `service/cron.rs:173` (+34 lines) |
| cron_complete_suffix | `cron/scheduler.rs:137` | `cron/scheduler.rs:156` (+19 lines) |
| codeterm_rule.md | `commands.rs:707` | `plugin/agent_claude/provider/integration.rs:345` |

Missing from the table entirely: `CODETERM_CLI` const.

A contributor using this table to trace where a prompt file goes will be off
by dozens of lines or sent to non-existent modules.

---

## 5. Mesh "any local command" is a dangerous over-promise

`codeterm_cli.md:105`:
> Any local command works on a remote peer by adding `--mesh TARGET`

I checked `cli/src/mesh_cli/api_map.rs`. The mesh relay is implemented as a
routing table that maps local commands to remote API calls. It's not "any
command" — it's an incomplete whitelist.

Specific problems found in the mesh API map:

**Silent mis-mapping:** `mem flush`, `run-worker`, `skill`, `worker`, and
`secret` subcommands are all mapped to `GET /api/mem/stats` instead of
rejecting with an error. An agent running `codeterm mem secret get --name x
--mesh peer` gets back mem stats, not an error. The agent proceeds thinking
it either got the secret or got nothing useful — either way, wrong behavior.

**Dropped fields on cron create:** The mesh mapping for `cron create`
(`api_map.rs:184-192`) maps only name, schedule, task, agent, workspace_id.
The local CLI handler also handles `once`, `jitter_max_ms`, `tz`,
`timeout_secs`, `creator_pane`. Remote cron creation silently loses all
these advanced options.

**Key name mismatch:** `api_map.rs:546-549` sends `entity_type` for graph
entity creation, but the local CLI at `cli/src/cmd/mem/mod.rs:454-458` sends
`"type"`. The two paths disagree on the JSON key name.

This isn't just a doc bug — it's an API consistency bug that the doc
exacerbates by claiming full coverage.

---

## 6. Cron jitter is additive only, not ± as documented

`codeterm_cli.md:161`:
> Jitter is on by default (±10% of interval)

I read the implementation at `cron/store/schedule.rs:88-113`:

```rust
let max = match jitter_max_ms {
    None => cmp::min(interval_ms / 4, 1_800_000),  // cap at 30 min
    Some(v) => cmp::min(v as u64, 1_800_000),
};
let offset = rng.gen_range(0..=max);
capped + offset  // always adds, never subtracts
```

The jitter is always a delay, never an advance. The word "±" is wrong.
For a 6-hour dreaming cron with default jitter, the window is 0-30 min
delay, not ±30 min. The upper bound is also 30 min cap, not 10% of interval
(which would be 36 min for 6h).

This is the same in `docs/MEMORY.md`, `docs/CONFIG.md`, and the dreaming
scheduler code comments. The ± wording appears in at least 4 places, zero of
them match the implementation.

---

## 7. Worker report format exists in docs, not in prompts

`docs/contributing/AGENTS.md` describes a structured template:

```
STATUS: done | blocked | partial
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything the Orchestrator should know
```

The actual `worker_suffix.md` is two lines:
```
When done, report by running: codeterm agent report "your summary"
```

There's no enforcement, no schema, no example. The template exists only in
the contributing doc that agents don't read. Workers produce unstructured
summaries. Orchestrators receive unstructured text.

If CodeTerm wants machine-parseable reports, the format needs to be in
`worker_suffix.md`, not in a contributor-facing doc.

---

## 8. Codex model switching doesn't work

`codeterm_cli.md:56`:
```
codeterm agent model sonnet|haiku|opus [--pane ID]
```

The CLI handler at `cli/src/cmd/agent.rs:275` prints:
> Codex /model is an interactive picker, not scriptable.

And the Codex provider at `plugin/agent_codex/provider.rs:373` explicitly
leaves `model_switching()` unsupported. The prompt doesn't scope this to
Claude.

An agent using Codex that reads the prompt will attempt this command and
get back either an error or an interactive picker that blocks in a headless
context.

---

## 9. `skills/codeterm/references/roles.md` is training agents on deleted features

This is a live skill file that agents actually read during operation. It
documents Lead spawning Workers, Lead role tasks, and a 4-level hierarchy.
The codebase has 2 levels. Any agent that reads this file will form a wrong
model of what it's allowed to do.

This is the highest-impact stale reference because it's consumed at runtime
by the very agents it describes, not just by human docs readers.

---

## 10. `bridge_mem.md` says "use the codeterm skill" — which one?

Three things are called "the codeterm skill":

1. **`seed/skill_codeterm.md`** (16 lines) — seeded into mem DB, returned by
   `mem search codeterm`. No CLI reference, no placeholder substitution.
2. **`skills/codeterm/SKILL.md`** (45 lines) — on-disk skill with
   `{codeterm_cli}` placeholder, substituted at seeder runtime.
3. **`codeterm_cli.md`** (335 lines) — not a skill, but the actual CLI
   reference that both the GA prompt and the on-disk skill include.

The seed stub is what `mem search` returns. It's 16 lines of basic commands.
The on-disk skill is what project agents load when they use the skill system.
The GA never loads any "codeterm skill" — it has `{codeterm_cli}` directly.

When `bridge_mem.md` says "use the codeterm skill", the agent that reads mem
gets the 16-line stub and misses the full reference. The GA already has the
full reference. Project agents that load the on-disk skill get the full
reference.

---

## 11. `bootstrap_instructions.md` uses Unix `rm` on a Windows system

Line 69: `rm bootstrap.md`

CodeTerm runs on Windows. The GA shell can be PowerShell, CMD, or Bash
depending on the system. `rm` is not portable. The file is at
`{general_dir}/bootstrap.md` — the prompt just says `bootstrap.md` without
a path, relying on the shell's CWD being correct.

---

## 12. Skill seeder has dead substitution code

`commands/skills.rs:89-101`:
```rust
if body.contains("{codeterm_cli}") {
    let rendered = body.replace("{codeterm_cli}", crate::prompts::CODETERM_CLI);
```

This works for `skills/codeterm/SKILL.md` but `seed/skill_codeterm.md`
doesn't have the placeholder. The mem seed stores the 16-line stub as-is.
The seeder writes the full substituted version to disk. Both exist, both are
called "codeterm skill", they contain different content.

The seeder's conditional guard means this code path IS exercised for the
on-disk skill. But the mem seed path (`mem/seed.rs`) includes the file raw
with no substitution. If someone adds `{codeterm_cli}` to the seed file,
it won't be substituted and `{codeterm_cli}` will appear literally in mem.

---

## 13. `docs/PROMPTS.md` extraction documentation is wrong on two numbers

Line 59-60:
> extraction worker spawned on `mem save` for items longer than ~80 chars
> body is truncated to 4000 chars before substitution

Reality from `auto_extract.rs`:
- Truncation is at **50,000 bytes**, not 4,000 (lines 27-42)
- Extraction is gated by `!skip_extract` flag, not by body length
- Kind filter excludes only `Summary` (`auto_extract.rs:18`), not the
  doc's listed subset

Two bad numbers, wrong trigger description.

---

## 14. `mem search` entity promise is too absolute

`mem_extraction.md:65` and `dreaming_rem.md:362`:
> mem search returns BOTH mem_items AND graph entities

Entity search through `mem search` works but depends on:
- Entity having a vector embedding (not all entities do)
- Entity being in the index
- Query matching entity text

This reads as a guarantee. When a worker `mem search`es and misses an entity,
the prompt doesn't tell it to also try `mem graph get <slug>` or
`mem graph related --id <id>`.

---

## 15. `docs/PROMPTS.md` says extraction fires for specific kinds — code disagrees

Doc says: "kinds: conversation, note, daily, learning, fact"

Code at `auto_extract.rs:17-19`:
```rust
fn should_extract(kind: MemKind) -> bool {
    !matches!(kind, MemKind::Summary)
}
```

All kinds except Summary trigger extraction. The doc's list is a subset,
not the actual rule. Missing: skill, rule, preference, peer, script.

---

## 16. `docs/PROMPTS.md` line 15 says `.claude/agents/*.md` is active — it's not

The doc says:
> Two locations, no third... `.claude/agents/*.md` — Claude Code agent
> definitions, read from disk on subagent spawn.

The `.claude/agents/` directory does not exist in this checkout. The CLAUDE.md
says the project intentionally moved role skills to `codeterm mem`.

The doc at line 24 also says:
> `.claude/agents/*.md` files are loaded by Claude Code itself... and bypass
> everything described above.

If the directory doesn't exist, either it needs to be removed from the doc or
the role skills need to be put back on disk.

---

## Summary of what hurts most

These are the findings that will cause wrong agent behavior, not just wrong docs:

1. **Lead role dead but advertised** — agents can be spawned as Lead and get
   zero suffix guidance
2. **Mesh over-promise** — "any command works" but many silently mis-map or
   lose fields
3. **Codex model switch impossible** — exactly what the prompt says to do
   doesn't work for Codex
4. **Roles.md trains agents wrong** — the live skill teaches a 4-level
   hierarchy that doesn't exist
5. **Jitter semantics** — agents relying on symmetrical ± get asymmetric delay

Everything else is doc decay, which is fixable with one pass through the files.

---

*This is my independent analysis. I didn't read the other version and I don't
know what it says. These are my findings from reading prompts → code → CLI.*

*— DeepSeek pass, 2026-05-25*
