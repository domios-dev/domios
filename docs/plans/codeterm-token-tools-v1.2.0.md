# CodeTerm Token Tools — v1.2.0 Plan

Goal: ship a small, vendor-neutral toolkit that **reduces token spend** of any agent
running under CodeTerm (Claude Code, Codex CLI, OpenCode, Cursor) without
replacing their auth or message stream. Every piece lives inside our existing
CLI + hooks + AGENTS.md infrastructure. No new binaries, no MCP server, no
extra processes.

Three deliverables:

1. **`codeterm snip <cmd>`** — runs `<cmd>`, filters its stdout/stderr through
   per-tool patterns, returns ~1–10 % of original size with the signal lines
   preserved (test fails, log heads/tails, diff hunk markers).
2. **`codeterm hook <event>`** — single subcommand that handles all hook
   events from all three CLIs. Reads provider's JSON from stdin, applies our
   logic, writes back. One codepath, four consumers.
3. **`codeterm map`** — aider-style symbol map of the active workspace.
   tree-sitter extracts defs/refs, PageRank ranks them, binary search fits the
   result into a token budget. Exposed both as a CLI and surfaced via hooks.

What this plan deliberately does **not** include (deferred to v1.3.0+):

- LLM-based prompt classification / cosine matching against intent clusters.
  Phase 5 covers history-only heuristics, which are language-agnostic and
  cheap. Cosine matching needs multilingual embedder + curated clusters and
  is a separate research stream.
- Cron-suggestion tips, mesh-suggestion tips, workspace-suggestion tips —
  same reason. Lands once the cosine layer is in.
- A full `codeterm-mcp` server. Re-evaluated and rejected: every tool we
  want is a stateless one-shot, CLI is sufficient, MCP just adds
  lifecycle complexity.

## Inspiration / where we steal patterns from

- **opencode-snip** (`references/opencode-snip/src/index.ts`, ~80 lines, MIT)
  — ideas for the Bash command-wrapper layer: env-var prefix preservation,
  shell-operator splitting, quote-aware pipe detection, unproxy-able-builtins
  list. Architecture mimicked, code written from scratch in Rust.
- **snip** (separate binary referenced by opencode-snip) — model for the
  per-tool truncation patterns. We re-implement in Rust as a CLI subcommand
  rather than a separate binary so it ships with CodeTerm.
- **aider** (`references/aider/aider/repomap.py`) — repo-map algorithm:
  tree-sitter tag extraction, PageRank with personalization vector, binary
  search to fit token budget, mtime-keyed disk cache.
- **roo-code** (`references/roo-code/src/core/condense/`) — non-destructive
  overlay pattern for our PreCompact hook (don't rewrite history, just
  inject hints).
- **plandex** (`references/plandex/app/server/model/plan/tell_context.go`)
  — path-mention auto-load on assistant output, and the "first user
  message is sacred" principle for our PreCompact hook.

---

## Phase 1 — `codeterm snip <cmd>`

Goal: a standalone subcommand we can use today from the shell, before any
hooks exist. Proves the truncation patterns and gives us measurement.

### Files

- `cli/src/cmd/snip.rs` (new) — dispatch + per-tool functions
- `cli/src/cmd/snip/cargo.rs`, `snip/go.rs`, `snip/git.rs`, `snip/node.rs`,
  `snip/python.rs` — one file per tool family, easy to grow
- `cli/src/cmd/snip/parser.rs` — quote-aware shell split (operator,
  pipe, env-prefix detection — Rust port of the opencode-snip parser)
- `cli/src/args/snip.rs` — clap subcommand + flags
- `cli/tests/snip_*.rs` — golden-fixture tests per tool

### Behaviour

```
codeterm snip cargo test -p mem
codeterm snip go test ./...
codeterm snip git log --oneline -100
codeterm snip git diff HEAD~5
codeterm snip npm test
codeterm snip pytest -v
```

Internals:

1. Parse argv into `(env_prefix, head_cmd, args[])`. Reject shell builtins
   (`cd`, `source`, `export`, …) — passthrough verbatim.
2. Spawn `<head_cmd> <args>` via `std::process::Command`, capture stdout +
   stderr (merged). Inherit exit code.
3. Dispatch on `head_cmd` to the matching truncator. Unknown command =
   identity (don't filter).
4. Print truncated output, exit with the original child's status code.

### Initial pattern set (one per tool family)

| Tool | Keep | Drop |
|------|------|------|
| `cargo test` / `cargo build` | `^test result:`, `FAILED`, `---- … stdout ----` + 20 lines, `error\[`, `warning:` (first 5 only) | `^test … ok`, `Compiling …`, `Finished …` |
| `go test` | `^--- FAIL:`, `FAIL`, `^=== RUN`, panic blocks | `^--- PASS:`, `^=== PAUSE`, `ok\s+pkg/` |
| `git log` | first 30 + `…` + last 5 | middle |
| `git diff` | `^diff --git`, `^@@`, ±10 lines around hunk header, file headers | hunk middles >50 lines |
| `npm test` / `jest` / `vitest` | `^FAIL`, `^●`, `Expected:` / `Received:` blocks | `^PASS`, ASCII spinner frames |
| `pytest` | `^FAILED`, `^ERROR`, summary block, traceback for first 3 failures | `^PASSED`, dots row |
| `npm run build` / `pnpm build` | `error TS\d+:`, `Failed to compile`, `^✗` | progress bars, `Compiling …`, watch-mode noise |

Unknown tool → passthrough. We grow the table as users surface gaps.

### Tests

Golden-fixture style: `cli/tests/fixtures/cargo-test-141pass-1fail.txt`
gets piped through `codeterm snip cargo test`, output asserted against
`cli/tests/fixtures/cargo-test-141pass-1fail.expected.txt`. One fixture per
tool per scenario (all-pass, mixed, all-fail, empty, panic). Fast, no
external deps.

### Acceptance

- `cargo test` golden run: ≥95 % byte reduction on green, ≥85 % on
  mixed-failure runs, ≥0 % on all-fail (preserves diagnostics).
- `git log -200`: deterministic 35-line output regardless of input size.
- 200+ unit tests, all passing.
- Cross-platform: works on Windows (git-bash), macOS, Linux. No POSIX-only
  syscalls. Shell parsing in pure Rust.

---

## Phase 2 — `codeterm hook <event>`

Goal: a single subcommand that handles every hook event from every
provider. Hooks fire frequently and must be fast (target: < 50 ms cold).

### Files

- `cli/src/cmd/hook.rs` (new) — entry point + dispatch on `<event>`
- `cli/src/cmd/hook/transcript.rs` — JSONL transcript reader (Claude
  format), tool-call extractor
- `cli/src/cmd/hook/events/{session_start,user_prompt_submit,pre_tool_use,pre_compact}.rs`
- `cli/src/cmd/hook/provider.rs` — normalizer that reads
  `CODETERM_HOOK_SOURCE` env (set by installer), parses provider-specific
  JSON, returns common `HookCtx` struct
- `cli/src/args/hook.rs` — clap subcommand

### Common contract

```
$ CODETERM_HOOK_SOURCE=claude codeterm hook user_prompt_submit < input.json
> output to stdout (or modified JSON if event requires)
```

Provider differences are absorbed by `provider.rs::normalize()`. Internal
events use a stable struct so per-event logic doesn't care which CLI fired.

### Per-event behaviour (Phase 2 minimum)

| Event | Phase 2 logic |
|-------|---------------|
| `session_start` | Stub — output "" so install doesn't break. Phase 5 fills in. |
| `user_prompt_submit` | Stub — output "". Phase 5 fills in. |
| `pre_tool_use` (Bash matcher) | Wrap `command` with `codeterm snip`. Same parser as Phase 1. Skip if already wrapped, skip builtins. |
| `pre_compact` | Stub — output "". Phase 5 fills in. |

### Acceptance

- `codeterm hook pre_tool_use` with Claude-format JSON `{"tool":"Bash","tool_input":{"command":"cargo test"}}` returns the same JSON with `tool_input.command` rewritten to `"codeterm snip cargo test"`.
- Same input with `CODETERM_HOOK_SOURCE=opencode` and OpenCode's JSON shape
  produces equivalent rewrite.
- Cold-start latency under 50 ms on a release build.

---

## Phase 3 — Provider install/uninstall

Goal: one-click "Install hooks" per provider, atomic edits, idempotent.

### Files

- `src-tauri/src/commands/hooks.rs` — extend with:
  - `check_codex_hooks` / `install_codex_hooks` / `uninstall_codex_hooks`
  - `check_opencode_hooks` / `install_opencode_hooks` / `uninstall_opencode_hooks`
- `src/components/Settings/AgentsTab.tsx` — extend Mac's per-provider
  Install card with hook badges + buttons for all three providers
- `src/components/Onboarding/Onboarding.tsx` — add a "Install token-saving
  hooks" step after the autostart toggle

### Marker

`codeterm-hook-v3` (bumped from `v2`). Stored as `_codeterm_hook_marker` in
each provider's settings file. On daemon start, if marker is older than
current code's expected version, silent re-install (with notification).

### Per-provider install layout

- **Claude** — JSON edit to `~/.claude/settings.json`, four hook events
  registered (Bash matcher on PreToolUse, `.*` on the others).
- **Codex** — TOML edit to `~/.codex/config.toml`, equivalent four events.
- **OpenCode** — JS plugin file emitted to
  `~/.config/opencode/plugins/codeterm-hooks.js` that just shells out to
  `codeterm hook <event>`. Or an entry in `opencode.json` plugins array if
  user has one. Both shapes detected.

### Acceptance

- Toggle on/off in Settings round-trips cleanly. Idempotent on repeated
  install. Uninstall leaves no markers in the file.
- After install, restart of provider CLI picks up the hook on next run.
- Onboarding wizard surfaces detected providers with checkboxes.

---

## Phase 4 — `codeterm map`

Goal: aider's repo-map without aider. Symbol-level project view that fits
in a token budget, replaces "agent does 12 Reads to understand the
codebase".

### Files

- `cli/src/cmd/map.rs` — entry point + clap flags
- `cli/src/cmd/map/extract.rs` — tree-sitter wrapper, language→query
  registry, returns `Vec<Tag>` per file
- `cli/src/cmd/map/queries/{rust,ts,js,py,go}.scm` — tree-sitter queries
  per language (defs and refs)
- `cli/src/cmd/map/graph.rs` — referencer→definer multi-edge graph + weights
- `cli/src/cmd/map/rank.rs` — PageRank over the graph with personalization
  vector (boosts user-mentioned + query-matching identifiers)
- `cli/src/cmd/map/render.rs` — render top-N tags as
  `path:line | signature_line` blocks
- `cli/src/cmd/map/budget.rs` — binary search to fit `--max-tokens`
- `cli/src/cmd/map/cache.rs` — SQLite-backed mtime cache, lives at
  `~/.codeterm/mem/repo_map.db`. Schema: `(workspace_path, file_path, mtime,
  tags_blob)`. Re-extract on mtime miss.

### CLI shape

```
codeterm map --query "auth login" --max-tokens 4000
codeterm map --paths src/api,src/db --max-tokens 6000
codeterm map --signatures src/lib/foo.rs           # single-file fold
codeterm map --refresh                              # invalidate cache
```

### Initial language coverage

Rust, TypeScript, JavaScript, Python, Go. Tree-sitter grammars already
vendored in `src-tauri/Cargo.lock` (we use them for syntax highlight). Add
new language by dropping a `.scm` file and a row in the registry.

### Acceptance

- 500-file Rust repo: `codeterm map --query "auth"` returns ranked map in
  ≤200 ms cold, ≤30 ms warm.
- 5000-file repo: ≤2 s cold, ≤80 ms warm.
- Token-budget rendering: produces map within ±5 % of `--max-tokens`.
- Cache invalidates correctly on file mtime change (one watcher hook in
  daemon's existing librarian).

---

## Phase 5 — Smart hook content

Now we fill the stubs from Phase 2 with real logic.

### `pre_compact`

Re-inject identity + workspace rules so condense doesn't drop them.

- Run `codeterm mem wake-up` (already exists, ~1500 tokens)
- Append workspace `AGENTS.md` and any active `<command>...</command>`
  blocks from the transcript (roo-code's `extractCommandBlocks` pattern)
- Output to stdout — Claude prepends to the post-compact context

### `user_prompt_submit`

Tool-history-only heuristics. No NLP.

- Read transcript (last 20 messages), extract assistant's tool calls
- If recent Bash calls match `cargo|go|npm|pnpm|pytest|jest|vitest|git log|git diff` and didn't go through `codeterm snip` → emit:
  `[codeterm tip: prefix test/build/log commands with 'codeterm snip' for ~99% output reduction]`
- If 3+ Read calls in the last 5 turns → emit:
  `[codeterm tip: try 'codeterm map --query "<term>"' to scan symbols across files in one shot]`
- If user prompt contains backticked paths → emit a stub block of those
  files' signatures via `codeterm map --signatures …` (plandex
  auto-load-on-mention)

Conditional emission. If no rule fires, output nothing — silent hook.

### `session_start`

Heavy one-time inject. Phase 5 wires it.

- `codeterm mem wake-up` (L0+L1 identity, ~1500 tokens)
- Workspace `<cwd>/.codeterm/repo.md` if present (openhands microagent
  pattern — long-term workspace facts written by user/agent)
- Tools cheatsheet (the same block currently sitting in CLAUDE.md)

### Acceptance

- Tip emission rate: at most 1 tip per turn. No tip if last turn already
  emitted the same tip.
- Hook latency: under 80 ms p95 (transcript read + regex + write).
- Self-test: hook output stays stable when re-run on identical transcript
  — no flicker, agent caches it cleanly.

---

## Phase 6 — Workspace `.codeterm/repo.md` discipline

Mirrors openhands' microagent pattern.

### Files

- `~/.codeterm/skills/repo-memory/SKILL.md` (new) — codifies the agent
  discipline: ask before saving, save general (not bug-specific)
  knowledge, point at `<workspace>/.codeterm/repo.md` for workspace-level
  facts, `codeterm mem save --kind fact` for cross-workspace.
- `src-tauri/src/service/agents_md.rs` — extend the existing AGENTS.md
  generator to also slurp `<workspace>/.codeterm/repo.md` if present and
  inline it under a `## Workspace memory` header.

### Acceptance

- Adding a `repo.md` to any workspace and reopening it surfaces the
  content in the auto-generated AGENTS.md / CLAUDE.md.
- Skill is discoverable via `codeterm mem search "workspace memory"`.
- Documentation in the skill explains exactly what to write there (build
  commands, lint rules, gotchas) vs what NOT to write (issue-specific
  notes).

---

## Sequencing and acceptance

1. **Phase 1** ships first as a regular CLI subcommand. Users (us) start
   manually prefixing test commands with `codeterm snip` for measurement.
   Goal: 1–2 days. Deliverable: PR landing the new subcommand + golden
   fixture suite.

2. **Phase 2** ships next. Without the hook installer (Phase 3), it's
   already callable manually for testing the JSON contracts. Goal:
   1 day. Deliverable: PR with the `hook` subcommand passing all three
   provider-format fixtures.

3. **Phase 3** wires it up to real providers. Once installed, every Bash
   tool call across all three CLIs auto-routes through Phase 1. Goal:
   2 days incl. UI. Deliverable: per-provider Install card + onboarding
   step.

4. **Phase 4** is the bigger lift. Tree-sitter graph + PageRank port from
   aider. Goal: 4–6 days. Deliverable: CLI works standalone first; hook
   integration deferred to Phase 5.

5. **Phase 5** fills the hook stubs once Phase 4 is done. Adds the
   content-aware reminders. Goal: 2 days.

6. **Phase 6** is documentation + skill polish. Goal: half a day.

Total v1.2.0 estimate: ~2 weeks of focused work, broken into PRs that
each ship independently behind their own user-facing toggle.

## Out of scope (v1.3.0+)

- Multilingual fastembed model + cosine prompt classification.
- Cron / mesh / workspace tip categories beyond Phase 5's tool-history
  set.
- Streaming stdout truncation in `codeterm snip` (currently buffers the
  whole child output before filtering — fine for tests/builds, would
  matter for very long-running commands).
- LLM-based summarization of tool output for the long-tail of unknown
  commands (passthrough is fine for now).
- "PostToolUse" auto-summarize-large-output hook (Claude Code issue
  #31279). Higher complexity, separate PR after Phase 5 baseline.

## Risks

- **Provider hook contract drift.** Anthropic/OpenAI/SST may change the
  hook JSON shape. Mitigation: provider normalization in `hook/provider.rs`
  is a single file, easy to patch. Add provider-version header check at
  install time.
- **Snip pattern false-cuts.** A test-runner format we haven't fixtured
  could lose signal lines. Mitigation: passthrough on unknown,
  community-PR-friendly pattern table.
- **Repo-map memory blowup on huge monorepos.** PageRank on >50k files
  needs care. Mitigation: per-language file cap with explicit warning,
  gitignore-aware traversal, lazy extraction — only files touched in
  recent N commits ranked first.
- **Cache invalidation race**. If user edits a file while map is rendering,
  we may emit stale signatures. Mitigation: snapshot mtime at extraction
  time, drop cache entries newer than the snapshot.

## Open questions

- Do we want the `pre_tool_use` Bash auto-wrap on by default, or
  opt-in? Auto is more impact, opt-in is safer. Lean: opt-in for v1.2.0,
  flip to default after a release of dogfood.
- `codeterm snip --strict` — exit non-zero if pattern matched zero lines
  (i.e. likely lost signal)? Or always pass through original on empty
  output? Lean: always passthrough.
- Cross-machine cache for `codeterm map`? Mesh-aware lookup? Probably
  v1.3.0.

## Related work in the codebase

- We already have `~/.claude/skills/` and `~/.codex/skills/` sync via
  `service::sync_watcher`. Skill files distribute automatically.
- `commands::install_claude_hooks` already exists for v2 markers. Phase 3
  bumps to v3 and adds Codex/OpenCode siblings.
- `mem::dreaming` runs nightly. Phase 4's repo-map cache should respect
  the same WAL discipline as mem.
- Mac's per-provider Install card (commits 285be4f / 1d8fae5 in
  release/1.1.13) is the perfect host for the hook install button.
