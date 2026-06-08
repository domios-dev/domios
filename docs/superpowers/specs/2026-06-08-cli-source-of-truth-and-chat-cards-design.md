# CLI Single Source of Truth + CodeTerm Chat Command Cards — Design

<!-- revision: 1 -->

Branch: `feat/system-plugins-intrinsic` (commit as `feat(cli)` / `feat(chat)`).

Two linked deliverables that share one artifact:

- **Part 1 — Chat command cards.** Render an agent's `codeterm …` Bash tool calls
  in the chat view as branded, semantic cards instead of raw shell blocks.
- **Part 2 — CLI single source of truth.** Make the clap command tree the one
  source from which docs, prompts, and the card templates are derived/validated,
  so agents editing the CLI can't silently drift these apart (a recurring past
  problem: new agents re-implemented CLI surface and duplicated guidance).

The seam between them is **`docs/cli-manifest.json`** — a machine-readable dump of
the clap tree. clap regenerates it (CI fails if stale); every other consumer
reads or is validated against it.

```
         clap tree (cli/src/args/*.rs)   ← the ultimate source
                    │ codeterm cli-manifest --json (regen)
                    ▼
         docs/cli-manifest.json  ← committed artifact, the seam
            ├──► docs/CLI.md            (auto-generated between markers)
            ├──► prompts/codeterm_cli.md (Rust doctor: every cmd/flag exists)
            └──► chat-ui card templates  (TS test: template paths ⊆ manifest)
```

---

## Part 1 — Chat command cards

### Goal

When a chat tool call is `Bash` and its command is a `codeterm …` invocation,
show a branded card (logo + verb-phrase + target + status) rather than the
generic "Bash" pill with the raw command. Unknown codeterm subcommands still get
the brand + a clean auto-summary. Anything that isn't a clean single codeterm
command falls through to today's generic Bash rendering untouched.

### Architecture

New self-contained module, provider-agnostic (any agent can shell `codeterm`),
so it lives in shared chat-ui — not in a provider plugin:

```
packages/chat-ui/src/codeterm/
  recognize.ts        — quote-aware tokenizer + binary detection + arg routing
  templates.ts        — declarative registry: subcommand-path → render spec
  template-paths.ts   — array of the subcommand paths templates.ts covers (the
                        list the TS test cross-checks against the manifest)
  CodetermCard.tsx     — the card component
  recognize.test.ts   — table tests (command string → parse + summary) + the
                        "template paths ⊆ manifest" assertion
```

`MessageBubble.tsx` gains exactly one early return, mirroring the existing
`TodoWrite` / `Edit`-`DiffView` special-case pattern (it already calls
`extractCommand(message)` for Bash): if `toolName === "Bash"` and
`recognize(command)` is non-null, render `<CodetermCard inv result />`.

### `recognize(command: string): CodetermInvocation | null`

- Quote-aware tokenizer (handles `"…"`, `'…'`, escaped spaces). No full shell
  grammar — we only need binary + subcommand chain + flags + positionals.
- Strip leading `KEY=val` env assignments (`CODETERM_API_PORT=7686 codeterm …`).
- Match the binary by **basename** of the first real token: `codeterm`,
  `codeterm-dev`, or an absolute/`~` path ending in one of those.
- If the codeterm call is buried after a pipe/`&&`/`;` (not the first command),
  return `null` — we won't misrepresent a compound command; it stays raw Bash.
- Return `{ binary, path: string[], flags: Record<string,string|true>,
  positionals: string[], raw }` where `path` is the subcommand chain
  (e.g. `["agent","spawn"]`, `["send-text"]`).

### `templates.ts`

Declarative registry keyed by the **longest matching** subcommand path:

```ts
interface CardTemplate {
  icon: ReactNode | string;
  title: (inv: CodetermInvocation) => string;      // "→ Sent to pane bda0e267"
  detail?: (inv: CodetermInvocation) => ReactNode;  // payload/body line
  status?: (result: string | undefined) => Status;  // parse "OK (sent)" → ✓ delivered
}
```

Templated families (per approved scope): `send-text`; `agent`
(spawn/report/list/model/restart/…); `mem` (save/search/get); `workspace` +
`pane`; `mesh`/`cron`/`keepawake`. **Fallback** (any other `codeterm …`): brand +
`path.join(" ")` as the title + raw args as detail. Every branch is still
expandable to the raw command + raw result.

`template-paths.ts` exports the flat list of covered paths; `recognize.test.ts`
asserts each one is present in `docs/cli-manifest.json` (so a template can never
point at a renamed/removed command).

### Card rendering (`CodetermCard.tsx`)

- **Collapsed:** CodeTerm glyph + verb-phrase title + target chip (pane id /
  workspace / mem id) + tiny status (✓/✗/…) derived from the tool result.
- **Expanded:** the raw `codeterm …` command (so power users can always read the
  exact call) + the raw result, reusing the existing `CodeBlock`.
- Status derivation is best-effort: known result strings (`OK (sent)` →
  `✓ delivered`) map to a status; otherwise no status pill.

### Cost

O(1) per `tool_call` message: one tokenize + registry lookup at render time. No
backend, no extra round-trips.

---

## Part 2 — CLI single source of truth

### 2.1 `codeterm cli-manifest [--json] [--write]`

New thin top-level command. Walks `Cli::command()` (clap's own tree) recursively
and emits, for every command/subcommand: `name`, `about`, `aliases`, `args`
(name, kind positional/flag, value-name, default, help), and nested
`subcommands`. `--json` (default) prints canonical, sorted, pretty JSON so the
artifact is diff-stable. `--write` regenerates both committed artifacts in place:
`docs/cli-manifest.json` and the auto-block of `docs/CLI.md`. **`codeterm
cli-manifest --write` is the single regen command** referenced everywhere below.

This is the only new runtime surface; everything else consumes its output.

### 2.2 `docs/cli-manifest.json` — the committed seam

`codeterm cli-manifest --write` writes the manifest to `docs/cli-manifest.json`.
A Rust freshness test (`cli_manifest_is_fresh`) regenerates in-memory and asserts
byte-equality with the committed file — **CI fails if the CLI changed without
running the regen command**.

### 2.3 `docs/CLI.md` — auto-generated full reference

A complete, always-correct CLI reference generated from the manifest between
`<!-- cli:auto-start -->` / `<!-- cli:auto-end -->` markers. Prose outside the
markers (intro, conventions) is hand-written and preserved. The same freshness
test asserts the generated block matches; drift fails CI. This retires the
stale, partial CLI section in `docs/API.md` — that section is replaced by a
one-line pointer to `docs/CLI.md`.

### 2.4 Prompt doctor (Rust test)

`prompts/codeterm_cli.md` is the agent-facing CLI guide and is currently
accurate — we keep it hand-written for wording, but guard it. A test
(`prompt_cli_commands_exist`) extracts every `codeterm …` command + flag token
from the prompt and asserts each resolves in the clap tree. Unknown command or
flag → test fails with the offending line. (Coverage direction — clap commands
missing from the prompt — is a non-failing report, since not every command needs
prompt coverage.)

### 2.5 Card-template doctor (TS test)

`recognize.test.ts` reads `docs/cli-manifest.json` and asserts every path in
`template-paths.ts` exists in the manifest. Cross-lang check with **no CLI build
needed** in the TS test — it reads the committed JSON. Ties the frontend to the
same source.

### 2.6 Concrete fixes folded in

- **Dead `Docs` arm** (`cli/src/main.rs:636`): removed — it's unreachable after
  the early-return at ~349.
- **Plugin manual-parse drift** (`parse_plugin_action` in `main.rs` duplicates
  the `PluginAction` tree): add `plugin_action_parser_covers_all_variants` test
  asserting the manual parser handles every `PluginAction`/`ChannelAction`
  variant, so adding a clap variant without wiring the parser fails CI. (We keep
  the interception parser — it exists for arg pass-through — but make drift loud.)
- **conductor/orchestrator terminology**: rename the variant to `SetOrchestrator`
  with `alias = "set-conductor"` to match the project's "Orchestrator → Worker"
  vocabulary (no external users; project policy allows breaking renames). Manifest
  regen + doc regen propagate it.

### 2.7 Cross-links (so humans/agents find the rule)

Short header pointers, each naming the guard test:

- `cli/src/args/cli.rs` top: "Source of truth for the CLI surface. On change run
  `codeterm cli-manifest --write` (updates docs/cli-manifest.json + docs/CLI.md);
  `prompt_cli_commands_exist` + chat-ui `recognize.test.ts` validate prompts and
  the chat cards against it."
- `prompts/codeterm_cli.md` top comment: "Validated against clap by
  `prompt_cli_commands_exist`. Don't document commands that don't exist."
- `packages/chat-ui/src/codeterm/templates.ts` top: "Paths validated against
  docs/cli-manifest.json by recognize.test.ts."
- `docs/contributing/cli.md` (new, short): the "Changing the CLI" checklist —
  edit clap → run regen → wording in prompt if behavior changed → tests green.

---

## Components & isolation

| Unit | Does | Depends on |
|------|------|-----------|
| `recognize.ts` | parse a shell string → CodetermInvocation\|null | nothing |
| `templates.ts` | invocation → render spec | recognize types |
| `CodetermCard.tsx` | render the card | templates, CodeBlock |
| `cli-manifest` cmd | clap tree → JSON | clap `Cli::command()` |
| manifest generator | write docs/cli-manifest.json + CLI.md block | cli-manifest |
| doctor tests | fail CI on drift | manifest, prompt, template-paths |

Each is independently testable; the card path is pure-render and never touches
the backend.

## Error handling

- `recognize` returns `null` on anything ambiguous (compound commands, non-codeterm,
  unparseable quotes) → safe fallback to today's Bash card.
- Unknown codeterm subcommand → fallback template, never an error.
- Manifest generator is deterministic (sorted) so freshness diffs are meaningful.

## Testing

- **TS:** `recognize.test.ts` — tokenizer edge cases (quotes, env-prefix, pipes),
  one assertion per template family (command → expected title/target), and the
  `paths ⊆ manifest` check. Run via `tsx` like the existing plugin tests.
- **Rust:** `cli_manifest_is_fresh` (regen == committed), `docs_cli_md_is_fresh`,
  `prompt_cli_commands_exist`, `plugin_action_parser_covers_all_variants`.
- CI wiring alongside `scripts/check-file-sizes.sh`.

## Out of scope (YAGNI)

- clap_complete / clap_mangen shell-completions & manpages (the manifest enables
  them later; not built now).
- Auto-generating `prompts/codeterm_cli.md` (kept hand-written for tone; only
  validated).
- Rendering codeterm commands buried inside compound shell lines.

## Rollback

Part 1 is additive (one early return + a new module) — revert the commit. Part 2
adds a command + tests + generated docs; revert restores hand-written docs. No
stored-state migration.
