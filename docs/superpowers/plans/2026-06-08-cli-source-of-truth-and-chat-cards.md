# CLI Source of Truth + Chat Command Cards — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the clap CLI tree the single source of truth (machine-readable manifest → generated docs + validated prompts + validated chat-card templates), and render `codeterm …` Bash tool calls in chat as branded semantic cards.

**Architecture:** A new `codeterm cli-manifest --write` command walks `Cli::command()` and writes `docs/cli-manifest.json` + an auto-block in `docs/CLI.md`. Rust unit tests fail CI when these are stale or when prompts reference non-existent commands. A frontend `packages/chat-ui/src/codeterm/` module recognizes codeterm invocations and renders cards; a TS test asserts its template paths exist in the committed manifest.

**Tech Stack:** Rust + clap 4 (derive, `CommandFactory`), serde_json; React 19 + TypeScript (chat-ui), `tsx` test runner.

Spec: `docs/superpowers/specs/2026-06-08-cli-source-of-truth-and-chat-cards-design.md`

---

## File Structure

**Part 2 — CLI source of truth (build first; manifest is the seam):**
- Create `cli/src/cmd/manifest.rs` — manifest model, clap-tree walker, JSON + CLI.md renderers, `--write`, freshness/doctor tests.
- Modify `cli/src/cmd/mod.rs` — `pub mod manifest;`.
- Modify `cli/src/args/cli.rs` — add `CliManifest { write: bool }` variant + source-of-truth header comment.
- Modify `cli/src/main.rs` — early short-circuit for `CliManifest`; remove dead `Docs` arm; plugin-coverage test.
- Modify `cli/src/args/agent.rs`, `cli/src/cmd/agent.rs` — `SetConductor` → `SetOrchestrator` (alias `set-conductor`).
- Create `docs/cli-manifest.json` — generated artifact (committed).
- Create `docs/CLI.md` — hand-written intro + generated auto-block.
- Modify `docs/API.md` — replace CLI section with pointer to `docs/CLI.md`.
- Modify `src-tauri/prompts/codeterm_cli.md` — add validated-by header comment.
- Create `docs/contributing/cli.md` — "Changing the CLI" checklist.
- Create `.github/workflows/cli-source-of-truth.yml` — runs cli cargo tests + chat-ui test.

**Part 1 — chat command cards:**
- Create `packages/chat-ui/src/codeterm/recognize.ts` — tokenizer + invocation parser.
- Create `packages/chat-ui/src/codeterm/templates.ts` — render-spec registry.
- Create `packages/chat-ui/src/codeterm/template-paths.ts` — covered subcommand paths.
- Create `packages/chat-ui/src/codeterm/CodetermCard.tsx` — the card.
- Create `packages/chat-ui/src/codeterm/recognize.test.ts` — tokenizer/template tests + paths⊆manifest.
- Modify `packages/chat-ui/src/MessageBubble.tsx` — one early return.
- Modify `scripts/plugin-tests.sh` — add the chat-ui test to `all`.

---

## PART 2 — CLI SOURCE OF TRUTH

### Task 1: Manifest model + `codeterm cli-manifest` (prints JSON)

**Files:**
- Create: `cli/src/cmd/manifest.rs`
- Modify: `cli/src/cmd/mod.rs`
- Modify: `cli/src/args/cli.rs` (add variant)
- Modify: `cli/src/main.rs` (early dispatch)

- [ ] **Step 1: Write the manifest model + walker + test (failing — module doesn't exist yet)**

Create `cli/src/cmd/manifest.rs`:

```rust
//! Machine-readable dump of the clap command tree — the single source of truth
//! consumed by docs/CLI.md, the prompt doctor, and the chat-card templates.
//! Regenerate committed artifacts with `codeterm cli-manifest --write`.

use clap::{Command, CommandFactory};
use serde::Serialize;

use crate::args::cli::Cli;

#[derive(Serialize, PartialEq, Debug)]
pub struct ManifestArg {
    pub name: String,
    pub positional: bool,
    pub value_name: Option<String>,
    pub default: Option<String>,
    pub help: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Serialize, PartialEq, Debug)]
pub struct ManifestCommand {
    pub name: String,
    pub about: Option<String>,
    pub aliases: Vec<String>,
    pub args: Vec<ManifestArg>,
    pub subcommands: Vec<ManifestCommand>,
}

/// Build the full manifest from the live clap tree.
pub fn build() -> ManifestCommand {
    walk(&Cli::command())
}

fn walk(cmd: &Command) -> ManifestCommand {
    let mut args: Vec<ManifestArg> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| ManifestArg {
            name: a.get_id().to_string(),
            positional: a.is_positional(),
            value_name: a.get_value_names().and_then(|v| v.first()).map(|s| s.to_string()),
            default: a
                .get_default_values()
                .first()
                .map(|s| s.to_string_lossy().to_string()),
            help: a.get_help().map(|s| s.to_string()),
            aliases: a
                .get_visible_aliases()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        })
        .collect();
    args.sort_by(|a, b| a.name.cmp(&b.name));

    let mut subcommands: Vec<ManifestCommand> =
        cmd.get_subcommands().map(walk).collect();
    subcommands.sort_by(|a, b| a.name.cmp(&b.name));

    let mut aliases: Vec<String> =
        cmd.get_visible_aliases().map(|s| s.to_string()).collect();
    aliases.sort();

    ManifestCommand {
        name: cmd.get_name().to_string(),
        about: cmd.get_about().map(|s| s.to_string()),
        aliases,
        args,
        subcommands,
    }
}

/// Canonical pretty JSON of the manifest (trailing newline, diff-stable).
pub fn to_json() -> String {
    let mut s = serde_json::to_string_pretty(&build()).expect("serialize manifest");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_core_commands() {
        let m = build();
        let names: Vec<&str> = m.subcommands.iter().map(|c| c.name.as_str()).collect();
        for expected in ["send-text", "agent", "mem", "pane", "cli-manifest"] {
            assert!(names.contains(&expected), "missing top-level command: {expected}");
        }
    }
}
```

- [ ] **Step 2: Register the module**

In `cli/src/cmd/mod.rs` add (alphabetical with siblings): `pub mod manifest;`

- [ ] **Step 3: Add the clap variant**

In `cli/src/args/cli.rs`, inside `enum Commands`, add (place near `Capabilities`):

```rust
    /// Dump the CLI command tree as JSON (source of truth for docs/prompts/cards)
    CliManifest {
        /// Regenerate committed artifacts (docs/cli-manifest.json + docs/CLI.md)
        #[arg(long)]
        write: bool,
    },
```

- [ ] **Step 4: Dispatch early (no daemon dependency)**

In `cli/src/main.rs`, next to the existing `Commands::Docs { .. }` early short-circuit (~line 349), add BEFORE it:

```rust
    // `cli-manifest` introspects the clap tree — no daemon dependency.
    if matches!(cli.command, Commands::CliManifest { .. }) {
        match cli.command {
            Commands::CliManifest { write } => cmd::manifest::handle(write),
            _ => unreachable!(),
        }
        return;
    }
```

Add a temporary `handle` to `manifest.rs` (Task 2 expands `--write`):

```rust
pub fn handle(write: bool) {
    if write {
        eprintln!("--write not implemented yet");
        std::process::exit(1);
    }
    print!("{}", to_json());
}
```

- [ ] **Step 5: Run tests + smoke the command**

Run: `cd cli && cargo test cmd::manifest::tests::manifest_has_core_commands`
Expected: PASS

Run: `cd cli && cargo run -- cli-manifest | head -20`
Expected: pretty JSON starting with `{ "name": "codeterm", ...`

- [ ] **Step 6: Commit**

```bash
git add cli/src/cmd/manifest.rs cli/src/cmd/mod.rs cli/src/args/cli.rs cli/src/main.rs
git commit -m "feat(cli): cli-manifest command dumps clap tree as JSON"
```

---

### Task 2: `--write` emits `docs/cli-manifest.json` + freshness test

**Files:**
- Modify: `cli/src/cmd/manifest.rs`
- Create: `docs/cli-manifest.json`

- [ ] **Step 1: Write the freshness test (failing — file doesn't exist)**

Add to `cli/src/cmd/manifest.rs` `tests`:

```rust
    const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/cli-manifest.json");

    #[test]
    fn cli_manifest_is_fresh() {
        let committed = std::fs::read_to_string(MANIFEST_PATH)
            .expect("docs/cli-manifest.json missing — run `codeterm cli-manifest --write`");
        assert_eq!(
            committed,
            to_json(),
            "docs/cli-manifest.json is stale — run `codeterm cli-manifest --write`"
        );
    }
```

- [ ] **Step 2: Implement `--write` for the JSON artifact**

Replace `handle` in `manifest.rs`:

```rust
fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
}

pub fn handle(write: bool) {
    if !write {
        print!("{}", to_json());
        return;
    }
    let manifest = repo_path("docs/cli-manifest.json");
    std::fs::write(&manifest, to_json()).expect("write cli-manifest.json");
    println!("wrote {}", manifest.display());
    // docs/CLI.md is wired in Task 3.
}
```

- [ ] **Step 3: Generate the artifact**

Run: `cd cli && cargo run -- cli-manifest --write`
Expected: `wrote .../docs/cli-manifest.json`

- [ ] **Step 4: Run the freshness test**

Run: `cd cli && cargo test cmd::manifest::tests::cli_manifest_is_fresh`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add cli/src/cmd/manifest.rs docs/cli-manifest.json
git commit -m "feat(cli): cli-manifest --write emits docs/cli-manifest.json + freshness test"
```

---

### Task 3: Generate `docs/CLI.md` auto-block + freshness test

**Files:**
- Modify: `cli/src/cmd/manifest.rs`
- Create: `docs/CLI.md`

- [ ] **Step 1: Create the hand-written shell of `docs/CLI.md`**

Create `docs/CLI.md`:

```markdown
# CodeTerm CLI Reference

<!-- revision: 1 -->

Complete command reference. The section between the markers below is
auto-generated from the clap command tree by `codeterm cli-manifest --write` —
do not edit it by hand. Conventions: `--pane` accepts alias `--id`; `--json` is
a global flag on every command.

<!-- cli:auto-start -->
<!-- cli:auto-end -->
```

- [ ] **Step 2: Write the markdown renderer + freshness test (failing)**

Add to `manifest.rs`:

```rust
/// Render the manifest as a nested markdown command reference.
pub fn to_markdown() -> String {
    let root = build();
    let mut out = String::new();
    for cmd in &root.subcommands {
        render_md(cmd, &[root.name.clone()], &mut out);
    }
    out
}

fn render_md(cmd: &ManifestCommand, path: &[String], out: &mut String) {
    let mut full = path.to_vec();
    full.push(cmd.name.clone());
    out.push_str(&format!("#### `{}`\n\n", full.join(" ")));
    if let Some(about) = &cmd.about {
        out.push_str(&format!("{about}\n\n"));
    }
    if !cmd.aliases.is_empty() {
        out.push_str(&format!("Aliases: {}\n\n", cmd.aliases.join(", ")));
    }
    for a in &cmd.args {
        let flag = if a.positional {
            format!("`<{}>`", a.name)
        } else {
            format!("`--{}`", a.name)
        };
        let help = a.help.as_deref().unwrap_or("");
        out.push_str(&format!("- {flag} — {help}\n"));
    }
    if !cmd.args.is_empty() {
        out.push('\n');
    }
    for sub in &cmd.subcommands {
        render_md(sub, &full, out);
    }
}
```

Add the freshness test:

```rust
    const CLI_MD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/CLI.md");
    const MD_START: &str = "<!-- cli:auto-start -->";
    const MD_END: &str = "<!-- cli:auto-end -->";

    #[test]
    fn docs_cli_md_is_fresh() {
        let md = std::fs::read_to_string(CLI_MD_PATH).expect("docs/CLI.md missing");
        let start = md.find(MD_START).expect("missing cli:auto-start marker") + MD_START.len();
        let end = md.find(MD_END).expect("missing cli:auto-end marker");
        let block = &md[start..end];
        let expected = format!("\n{}\n", to_markdown());
        assert_eq!(block, expected, "docs/CLI.md auto-block stale — run `codeterm cli-manifest --write`");
    }
```

- [ ] **Step 3: Extend `--write` to splice the auto-block**

In `manifest.rs::handle`, after writing the JSON, add:

```rust
    let cli_md = repo_path("docs/CLI.md");
    let md = std::fs::read_to_string(&cli_md).expect("read docs/CLI.md");
    let start = md.find("<!-- cli:auto-start -->").expect("auto-start marker")
        + "<!-- cli:auto-start -->".len();
    let end = md.find("<!-- cli:auto-end -->").expect("auto-end marker");
    let spliced = format!("{}\n{}\n{}", &md[..start], to_markdown(), &md[end..]);
    std::fs::write(&cli_md, spliced).expect("write docs/CLI.md");
    println!("wrote {}", cli_md.display());
```

- [ ] **Step 4: Generate + verify**

Run: `cd cli && cargo run -- cli-manifest --write`
Expected: writes both files.

Run: `cd cli && cargo test cmd::manifest::tests::docs_cli_md_is_fresh`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add cli/src/cmd/manifest.rs docs/CLI.md
git commit -m "feat(cli): generate docs/CLI.md auto-block from manifest + freshness test"
```

---

### Task 4: Prompt doctor test

**Files:**
- Modify: `cli/src/cmd/manifest.rs` (add doctor helpers + test)

- [ ] **Step 1: Add a path/flag lookup helper to the model**

Add to `manifest.rs` (non-test):

```rust
impl ManifestCommand {
    /// Resolve a subcommand path (e.g. ["agent","spawn"]) honoring aliases.
    pub fn resolve(&self, path: &[&str]) -> Option<&ManifestCommand> {
        let mut cur = self;
        for seg in path {
            cur = cur.subcommands.iter().find(|c| {
                c.name == *seg || c.aliases.iter().any(|a| a == seg)
            })?;
        }
        Some(cur)
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.args.iter().any(|a| {
            !a.positional && (a.name == flag || a.aliases.iter().any(|al| al == flag))
        })
    }
}
```

- [ ] **Step 2: Write the doctor test (failing if the prompt names a bad command)**

Add to `manifest.rs` `tests`:

```rust
    const PROMPT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src-tauri/prompts/codeterm_cli.md");

    // Extract `codeterm <subcmd...>` heads from prompt lines/code spans.
    #[test]
    fn prompt_cli_commands_exist() {
        let root = build();
        let text = std::fs::read_to_string(PROMPT_PATH).expect("read codeterm_cli.md");
        let mut bad: Vec<String> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('`').trim_start_matches("$ ");
            let Some(rest) = line.strip_prefix("codeterm ") else { continue };
            // Walk leading non-flag tokens as the subcommand path.
            let mut path: Vec<&str> = Vec::new();
            for tok in rest.split_whitespace() {
                if tok.starts_with('-') || tok.starts_with('"') || tok.contains('=') {
                    break;
                }
                // Stop at obvious argument placeholders (UPPERCASE / <...>).
                if tok.starts_with('<') || tok.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    break;
                }
                path.push(tok);
                // Most commands are depth 1-2; cap to avoid eating positionals.
                if root.resolve(&path).is_some() && path.len() >= 1 {
                    // keep extending only while still resolvable
                    continue;
                }
            }
            // Trim path back to the longest resolvable prefix.
            while !path.is_empty() && root.resolve(&path).is_none() {
                path.pop();
            }
            if path.is_empty() {
                bad.push(format!("unrecognized: {line}"));
            }
        }
        assert!(bad.is_empty(), "prompt references unknown CLI commands:\n{}", bad.join("\n"));
    }
```

Note: heuristic deliberately conservative — it only flags a line whose first token after `codeterm` resolves to no command at all. Tighten later if needed.

- [ ] **Step 3: Run it**

Run: `cd cli && cargo test cmd::manifest::tests::prompt_cli_commands_exist`
Expected: PASS (audit confirmed the prompt is currently accurate). If it fails, the failure lists the offending line — fix the prompt, not the test.

- [ ] **Step 4: Commit**

```bash
git add cli/src/cmd/manifest.rs
git commit -m "test(cli): doctor — prompt CLI commands must exist in clap tree"
```

---

### Task 5: Plugin-action variant coverage test

**Files:**
- Modify: `cli/src/main.rs` (add test near existing tests ~line 872+)

- [ ] **Step 1: Write the coverage test (compile-forces awareness + parser check)**

Add to the `#[cfg(test)] mod tests` in `cli/src/main.rs`:

```rust
    use crate::args::plugin::PluginAction;

    // Compile guard: adding a PluginAction variant breaks this match until the
    // author acknowledges it here AND wires parse_plugin_action below.
    #[allow(dead_code)]
    fn _plugin_variants_exhaustive(a: PluginAction) {
        match a {
            PluginAction::List
            | PluginAction::Install { .. }
            | PluginAction::Update { .. }
            | PluginAction::UpdateSystem { .. }
            | PluginAction::Remove { .. }
            | PluginAction::Channel { .. } => {}
        }
    }

    #[test]
    fn plugin_parser_handles_each_subcommand() {
        // Every primary subcommand string the manual parser must accept.
        for (cmd, extra) in [
            ("list", vec![]),
            ("install", vec!["some-id"]),
            ("update", vec!["some-id"]),
            ("update-system", vec![]),
            ("remove", vec!["some-id"]),
            ("channel", vec!["list"]),
        ] {
            let mut args = vec![cmd.to_string()];
            args.extend(extra.into_iter().map(String::from));
            assert!(
                super::parse_plugin_action(args).is_ok(),
                "parse_plugin_action rejected subcommand `{cmd}`"
            );
        }
    }
```

(Adjust the exhaustive match arms to the actual `PluginAction` variants in `cli/src/args/plugin.rs` if they differ; the compile error will name any missing variant.)

- [ ] **Step 2: Run it**

Run: `cd cli && cargo test plugin_parser_handles_each_subcommand`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add cli/src/main.rs
git commit -m "test(cli): plugin manual parser must cover every PluginAction variant"
```

---

### Task 6: Remove dead `Docs` match arm

**Files:**
- Modify: `cli/src/main.rs:633-639`

- [ ] **Step 1: Delete the unreachable arm**

Remove this block (the `Docs` is already handled by the early short-circuit at ~line 349):

```rust
        Commands::Docs { name, full, head } => {
            cmd::docs::handle(name, full, head);
            return;
        }
```

- [ ] **Step 2: Verify it still builds (match is now satisfied by the early return)**

Run: `cd cli && cargo build`
Expected: builds clean (no non-exhaustive-match error, because `Docs` returns early before this `match`).

If the compiler reports `Docs` as non-exhaustive here, replace the deleted arm with `Commands::Docs { .. } => unreachable!("handled before socket connect"),` to mirror the `Config`/`System` pattern already in this match.

- [ ] **Step 3: Commit**

```bash
git add cli/src/main.rs
git commit -m "refactor(cli): drop dead Docs match arm (handled by early short-circuit)"
```

---

### Task 7: Rename `SetConductor` → `SetOrchestrator`

**Files:**
- Modify: `cli/src/args/agent.rs:6-7`
- Modify: `cli/src/cmd/agent.rs:100`
- Modify: `docs/cli-manifest.json`, `docs/CLI.md` (regen)

- [ ] **Step 1: Rename the clap variant, keep old name as alias**

In `cli/src/args/agent.rs`:

```rust
    /// Mark a pane as orchestrator (top-level, can spawn workers)
    #[command(alias = "set-conductor")]
    SetOrchestrator {
        #[arg(long)]
        pane: Option<String>,
    },
```

- [ ] **Step 2: Update the handler match**

In `cli/src/cmd/agent.rs:100` change `AgentAction::SetConductor { pane }` to `AgentAction::SetOrchestrator { pane }`. Leave the handler body (it already speaks "Orchestrator").

- [ ] **Step 3: Build + regenerate artifacts**

Run: `cd cli && cargo build`
Expected: clean.

Run: `cd cli && cargo run -- cli-manifest --write`
Expected: updates both docs (now shows `set-orchestrator` with alias `set-conductor`).

- [ ] **Step 4: Run freshness tests**

Run: `cd cli && cargo test cmd::manifest::tests`
Expected: PASS (all freshness/doctor tests).

- [ ] **Step 5: Commit**

```bash
git add cli/src/args/agent.rs cli/src/cmd/agent.rs docs/cli-manifest.json docs/CLI.md
git commit -m "refactor(cli): rename set-conductor -> set-orchestrator (alias kept)"
```

---

### Task 8: Cross-link comments + docs pointers

**Files:**
- Modify: `cli/src/args/cli.rs` (header comment)
- Modify: `src-tauri/prompts/codeterm_cli.md` (header)
- Modify: `docs/API.md` (replace CLI section with pointer; bump revision)
- Create: `docs/contributing/cli.md`

- [ ] **Step 1: Add the source-of-truth header to `cli/src/args/cli.rs`**

At the very top of the file (above `use clap::...`):

```rust
//! SOURCE OF TRUTH for the CodeTerm CLI surface.
//! On any change here run `codeterm cli-manifest --write` to regenerate
//! docs/cli-manifest.json + docs/CLI.md. The tests `cli_manifest_is_fresh`,
//! `docs_cli_md_is_fresh`, and `prompt_cli_commands_exist` (cli/src/cmd/manifest.rs)
//! plus the chat-ui `recognize.test.ts` fail CI if anything drifts.
//! See docs/contributing/cli.md.
```

- [ ] **Step 2: Add the validated-by header to the prompt**

At the top of `src-tauri/prompts/codeterm_cli.md` (after the H1, as an HTML comment so it doesn't render):

```markdown
<!-- Validated against the clap tree by `prompt_cli_commands_exist`
     (cli/src/cmd/manifest.rs). Do not document commands or flags that don't
     exist. Full generated reference: docs/CLI.md. -->
```

- [ ] **Step 3: Replace the stale CLI section in `docs/API.md`**

Find the `## CLI` section in `docs/API.md` and replace its body with:

```markdown
## CLI

The complete CLI reference is auto-generated from the clap command tree:
see **`docs/CLI.md`** (regenerated by `codeterm cli-manifest --write`). The
machine-readable tree lives in `docs/cli-manifest.json`.
```

Bump the `<!-- revision: N -->` tag at the bottom of `docs/API.md` by 1.

- [ ] **Step 4: Create the contributing checklist**

Create `docs/contributing/cli.md`:

```markdown
# Changing the CLI

<!-- revision: 1 -->

The clap tree in `cli/src/args/*.rs` is the single source of truth.

When you add/rename/remove a command or flag:

1. Edit the clap definition in `cli/src/args/`.
2. Run `codeterm cli-manifest --write` — regenerates `docs/cli-manifest.json`
   and the auto-block of `docs/CLI.md`.
3. If agent-facing behavior changed, update the wording in
   `src-tauri/prompts/codeterm_cli.md` (it is validated, not generated).
4. `cargo test -p codeterm-cli cmd::manifest` must be green
   (`cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, `prompt_cli_commands_exist`).
5. If you added a chat-card template, it is checked by chat-ui `recognize.test.ts`.

CI (`.github/workflows/cli-source-of-truth.yml`) runs all of the above.
```

- [ ] **Step 5: Verify nothing broke**

Run: `cd cli && cargo test cmd::manifest::tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add cli/src/args/cli.rs src-tauri/prompts/codeterm_cli.md docs/API.md docs/contributing/cli.md
git commit -m "docs(cli): cross-link clap source-of-truth to docs/prompts + contributing checklist"
```

---

## PART 1 — CHAT COMMAND CARDS

### Task 9: `recognize.ts` — tokenizer + invocation parser

**Files:**
- Create: `packages/chat-ui/src/codeterm/recognize.ts`
- Create: `packages/chat-ui/src/codeterm/recognize.test.ts`

- [ ] **Step 1: Write the failing test**

Create `packages/chat-ui/src/codeterm/recognize.test.ts`:

```ts
// Run: npx tsx packages/chat-ui/src/codeterm/recognize.test.ts
import { recognize } from "./recognize";

let passed = 0, failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual), e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}
function ok(c: boolean, name: string) { eq(c, true, name); }

const inv = recognize('codeterm send-text --pane bda0e267 "Понял, буду"');
eq(inv?.path, ["send-text"], "send-text path");
eq(inv?.flags.pane, "bda0e267", "send-text --pane flag");
eq(inv?.positionals, ["Понял, буду"], "send-text positional payload");

eq(recognize('codeterm agent spawn claude --task "go"')?.path, ["agent", "spawn"], "agent spawn path");
eq(recognize("CODETERM_API_PORT=7686 codeterm pane list")?.path, ["pane", "list"], "env-prefix stripped");
ok(recognize("~/.codeterm/bin/codeterm mem search foo") !== null, "abs path binary recognized");
ok(recognize("ls -la") === null, "non-codeterm → null");
ok(recognize('echo hi | codeterm send-text "x"') === null, "buried after pipe → null");
ok(recognize("codeterm-dev pane list") !== null, "codeterm-dev recognized");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
```

- [ ] **Step 2: Run it (fails — module missing)**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: FAIL (cannot find `./recognize`).

- [ ] **Step 3: Implement `recognize.ts`**

Create `packages/chat-ui/src/codeterm/recognize.ts`:

```ts
// Recognize `codeterm …` shell invocations in a Bash tool command string.
// Paths validated against docs/cli-manifest.json by recognize.test.ts.

export interface CodetermInvocation {
  binary: string;
  path: string[];                       // subcommand chain, e.g. ["agent","spawn"]
  flags: Record<string, string | true>; // --pane bda → { pane: "bda" }
  positionals: string[];
  raw: string;
}

const BINARIES = new Set(["codeterm", "codeterm-dev"]);

/** Split a shell-ish string into tokens, honoring single/double quotes. */
export function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  let has = false;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch; has = true;
    } else if (ch === " " || ch === "\t" || ch === "\n") {
      if (has) { tokens.push(cur); cur = ""; has = false; }
    } else {
      cur += ch; has = true;
    }
  }
  if (has) tokens.push(cur);
  return tokens;
}

function basename(tok: string): string {
  const slash = Math.max(tok.lastIndexOf("/"), tok.lastIndexOf("\\"));
  return slash >= 0 ? tok.slice(slash + 1) : tok;
}

export function recognize(command: string): CodetermInvocation | null {
  if (!command) return null;
  // Reject compound commands — we won't misrepresent a chained line.
  if (/[|;]|&&|\|\|/.test(command)) return null;

  const tokens = tokenize(command);
  let i = 0;
  // Strip leading KEY=val env assignments.
  while (i < tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[i])) i++;
  if (i >= tokens.length) return null;

  const binary = basename(tokens[i]);
  if (!BINARIES.has(binary)) return null;
  i++;

  const path: string[] = [];
  const flags: Record<string, string | true> = {};
  const positionals: string[] = [];
  let seenFlag = false;

  for (; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.startsWith("--")) {
      seenFlag = true;
      const eq = tok.indexOf("=");
      if (eq > 0) { flags[tok.slice(2, eq)] = tok.slice(eq + 1); continue; }
      const key = tok.slice(2);
      const next = tokens[i + 1];
      if (next != null && !next.startsWith("-")) { flags[key] = next; i++; }
      else flags[key] = true;
    } else if (tok.startsWith("-") && tok.length > 1) {
      seenFlag = true;
      flags[tok.slice(1)] = true;
    } else if (!seenFlag && path.length < 3) {
      // Bare words before any flag form the subcommand chain (depth-capped).
      path.push(tok);
    } else {
      positionals.push(tok);
    }
  }

  if (path.length === 0) return null;
  return { binary, path, flags, positionals, raw: command };
}
```

Note: the depth cap (3) over-captures positionals into `path` (e.g. `send-text "text"` → path `["send-text","text"]`). Task 10's templates resolve the longest known path; Task 11's manifest test guards real command paths. To keep `recognize` honest, Task 11 trims `path` against the manifest — but for the card it's enough that the FIRST path segment is the command. Keep `path[0]` as the command key; later segments are advisory.

- [ ] **Step 4: Run the test**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: the `send-text positional payload` case currently FAILS because `"Понял, буду"` lands in `path`, not `positionals`.

- [ ] **Step 5: Fix path/positional split using a known-commands hint**

Refine: pass an optional set of known subcommands so `recognize` stops growing `path` once it leaves the command tree. Add a parameter and a default minimal set (full validation is the manifest test's job):

```ts
const KNOWN_SUBCOMMANDS: Record<string, Set<string>> = {
  agent: new Set(["spawn", "report", "list", "model", "restart", "providers", "models", "reports", "compact", "stop", "set-orchestrator", "link", "unlink"]),
  mem: new Set(["save", "search", "get", "delete", "skill", "graph", "worker"]),
  workspace: new Set(["create", "list", "focus", "rename", "close", "current", "move-tab"]),
  pane: new Set(["list", "new", "focus", "close", "rename", "exec", "status", "search", "stop", "current"]),
  mesh: new Set(["send", "exec", "output", "panes", "peers", "status", "connect"]),
  cron: new Set(["create", "list", "edit", "delete", "trigger", "history"]),
  keepawake: new Set(["start", "stop", "status"]),
};

// inside recognize, replace the `!seenFlag && path.length < 3` branch:
    } else if (!seenFlag && path.length === 0) {
      path.push(tok);            // first bare word = top command
    } else if (!seenFlag && path.length === 1 && KNOWN_SUBCOMMANDS[path[0]]?.has(tok)) {
      path.push(tok);            // second word only if a known subcommand
    } else {
      positionals.push(tok);
    }
```

- [ ] **Step 6: Run the test again**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: PASS (all cases).

- [ ] **Step 7: Commit**

```bash
git add packages/chat-ui/src/codeterm/recognize.ts packages/chat-ui/src/codeterm/recognize.test.ts
git commit -m "feat(chat): recognize codeterm CLI invocations in Bash tool commands"
```

---

### Task 10: `templates.ts` + `template-paths.ts`

**Files:**
- Create: `packages/chat-ui/src/codeterm/templates.ts`
- Create: `packages/chat-ui/src/codeterm/template-paths.ts`
- Modify: `packages/chat-ui/src/codeterm/recognize.test.ts` (template assertions)

- [ ] **Step 1: Add failing template assertions to the test**

Append to `recognize.test.ts`:

```ts
import { renderTemplate } from "./templates";

const sent = recognize('codeterm send-text --pane bda0e267 "hi there"')!;
eq(renderTemplate(sent).title, "Sent to pane bda0e267", "send-text title");

const spawn = recognize('codeterm agent spawn claude --task "do x" --workspace ws1')!;
eq(renderTemplate(spawn).title, "Spawned claude", "agent spawn title");

const unknown = recognize("codeterm cron list --json")!;
eq(renderTemplate(unknown).title, "cron list", "unknown → fallback title");
```

- [ ] **Step 2: Run (fails — templates missing)**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: FAIL (cannot find `./templates`).

- [ ] **Step 3: Implement `template-paths.ts`**

Create `packages/chat-ui/src/codeterm/template-paths.ts`:

```ts
// Subcommand paths the card has hand-written templates for. recognize.test.ts
// asserts each exists in docs/cli-manifest.json so a template can't point at a
// renamed/removed command.
export const TEMPLATE_PATHS: string[][] = [
  ["send-text"],
  ["agent", "spawn"],
  ["agent", "report"],
  ["agent", "list"],
  ["mem", "save"],
  ["mem", "search"],
  ["mem", "get"],
  ["workspace", "create"],
  ["pane", "focus"],
  ["mesh", "send"],
  ["cron", "create"],
  ["keepawake", "start"],
];
```

- [ ] **Step 4: Implement `templates.ts`**

Create `packages/chat-ui/src/codeterm/templates.ts`:

```ts
import type { ReactNode } from "react";
import type { CodetermInvocation } from "./recognize";

export interface CardStatus { ok: boolean; label: string; }
export interface RenderedCard {
  icon: string;
  title: string;
  detail?: string;
  status?: CardStatus;
}

function pane(inv: CodetermInvocation): string {
  const p = inv.flags.pane;
  return typeof p === "string" ? p : "current pane";
}

function deliveredStatus(result?: string): CardStatus | undefined {
  if (!result) return undefined;
  if (/\bOK\b|\(sent\)|delivered/i.test(result)) return { ok: true, label: "delivered" };
  if (/error|fail|not found/i.test(result)) return { ok: false, label: "failed" };
  return undefined;
}

type TemplateFn = (inv: CodetermInvocation, result?: string) => RenderedCard;

const TEMPLATES: Record<string, TemplateFn> = {
  "send-text": (inv, result) => ({
    icon: "→",
    title: `Sent to pane ${pane(inv)}`,
    detail: inv.positionals[0],
    status: deliveredStatus(result),
  }),
  "agent spawn": (inv) => ({
    icon: "🤖",
    title: `Spawned ${inv.positionals[0] ?? "claude"}`,
    detail: typeof inv.flags.task === "string" ? inv.flags.task : undefined,
  }),
  "agent report": (inv) => ({ icon: "📨", title: "Reported to parent", detail: inv.positionals[0] }),
  "agent list": () => ({ icon: "🤖", title: "Listed agents" }),
  "mem save": (inv) => ({ icon: "🧠", title: `Saved memory${typeof inv.flags.title === "string" ? `: ${inv.flags.title}` : ""}`, detail: inv.positionals[0] }),
  "mem search": (inv) => ({ icon: "🔍", title: `Searched mem`, detail: inv.positionals[0] }),
  "mem get": (inv) => ({ icon: "📖", title: `Recalled ${inv.positionals[0] ?? ""}` }),
  "workspace create": (inv) => ({ icon: "🪟", title: `Created workspace ${inv.positionals[0] ?? ""}` }),
  "pane focus": (inv) => ({ icon: "→", title: `Focused pane ${inv.positionals[0] ?? pane(inv)}` }),
  "mesh send": (inv) => ({ icon: "🛰", title: `Mesh → ${inv.positionals[0] ?? ""}`, detail: inv.positionals[1] }),
  "cron create": (inv) => ({ icon: "⏰", title: `Scheduled ${inv.positionals[0] ?? "job"}` }),
  "keepawake start": () => ({ icon: "☕", title: "Keep-awake started" }),
};

/** Pick the longest matching template, else a branded fallback. */
export function renderTemplate(inv: CodetermInvocation, result?: string): RenderedCard {
  const two = inv.path.slice(0, 2).join(" ");
  const one = inv.path[0];
  const fn = TEMPLATES[two] ?? TEMPLATES[one];
  if (fn) return fn(inv, result);
  return {
    icon: "⬡",
    title: inv.path.join(" "),
    detail: inv.positionals.join(" ") || undefined,
    status: deliveredStatus(result),
  };
}
```

- [ ] **Step 5: Run the test**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: PASS (template assertions green).

- [ ] **Step 6: Commit**

```bash
git add packages/chat-ui/src/codeterm/templates.ts packages/chat-ui/src/codeterm/template-paths.ts packages/chat-ui/src/codeterm/recognize.test.ts
git commit -m "feat(chat): declarative codeterm command card templates"
```

---

### Task 11: Template-paths ⊆ manifest cross-check

**Files:**
- Modify: `packages/chat-ui/src/codeterm/recognize.test.ts`

- [ ] **Step 1: Add the subset assertion reading the committed manifest**

Append to `recognize.test.ts` (uses fs so no CLI build needed):

```ts
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { TEMPLATE_PATHS } from "./template-paths";

const manifestPath = fileURLToPath(new URL("../../../../docs/cli-manifest.json", import.meta.url));
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

function resolve(node: any, path: string[]): boolean {
  let cur = node;
  for (const seg of path) {
    cur = (cur.subcommands ?? []).find(
      (c: any) => c.name === seg || (c.aliases ?? []).includes(seg),
    );
    if (!cur) return false;
  }
  return true;
}

for (const path of TEMPLATE_PATHS) {
  ok(resolve(manifest, path), `template path exists in manifest: ${path.join(" ")}`);
}
```

- [ ] **Step 2: Run it**

Run: `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts`
Expected: PASS (every template path resolves in `docs/cli-manifest.json`). If a path fails, either fix `template-paths.ts` or the manifest is stale (`codeterm cli-manifest --write`).

- [ ] **Step 3: Commit**

```bash
git add packages/chat-ui/src/codeterm/recognize.test.ts
git commit -m "test(chat): card template paths must exist in cli-manifest"
```

---

### Task 12: `CodetermCard.tsx`

**Files:**
- Create: `packages/chat-ui/src/codeterm/CodetermCard.tsx`

- [ ] **Step 1: Implement the card (no separate test — covered by Task 13 visual + templates unit tests)**

Create `packages/chat-ui/src/codeterm/CodetermCard.tsx`:

```tsx
import type { CodetermInvocation } from "./recognize";
import { renderTemplate } from "./templates";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  expanded: boolean;
  onToggle: () => void;
}

export function CodetermCard({ inv, result, expanded, onToggle }: Props) {
  const card = renderTemplate(inv, result);
  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
      <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
        <button
          onClick={onToggle}
          className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
          style={{ background: "transparent", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
        >
          <span className="flex-shrink-0" style={{ color: "var(--ct-accent, #6ea8fe)" }}>⬡</span>
          <span className="font-medium" style={{ color: "var(--ct-accent, #6ea8fe)" }}>CodeTerm</span>
          <span className="flex-shrink-0">{card.icon}</span>
          <span className="truncate">{card.title}</span>
          {card.status && (
            <span className="flex-shrink-0" style={{ color: card.status.ok ? "var(--ct-ok, #41d18a)" : "var(--ct-err, #ff6b6b)" }}>
              {card.status.ok ? "✓" : "✗"} {card.status.label}
            </span>
          )}
        </button>
        {card.detail && !expanded && (
          <div className="px-2.5 py-1 text-xs truncate" style={{ color: "var(--ct-muted-fg)" }}>{card.detail}</div>
        )}
        {expanded && (
          <div className="mt-1 rounded-md border px-2.5 py-1.5 text-xs" style={{ borderColor: "var(--ct-border)" }}>
            <pre className="whitespace-pre-wrap break-all" style={{ color: "var(--ct-muted-fg)" }}>{inv.raw}</pre>
            {result && (
              <pre className="mt-1 whitespace-pre-wrap break-all" style={{ color: "var(--ct-fg)" }}>{result}</pre>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm -s exec tsc --noEmit`
Expected: no new errors from the codeterm module.

- [ ] **Step 3: Commit**

```bash
git add packages/chat-ui/src/codeterm/CodetermCard.tsx
git commit -m "feat(chat): CodetermCard component"
```

---

### Task 13: Wire the card into `MessageBubble`

**Files:**
- Modify: `packages/chat-ui/src/MessageBubble.tsx` (imports + one early return after the TodoWrite block, ~line 1800)

- [ ] **Step 1: Add imports near the top of MessageBubble.tsx**

```tsx
import { recognize } from "./codeterm/recognize";
import { CodetermCard } from "./codeterm/CodetermCard";
```

- [ ] **Step 2: Add the early return**

Immediately after the TodoWrite `return (...)` block (before `const detailText = ...`), insert:

```tsx
    // CodeTerm CLI command — render a branded semantic card.
    if (command) {
      const inv = recognize(command);
      if (inv) {
        return (
          <CodetermCard
            inv={inv}
            result={message.toolResult}
            expanded={expanded}
            onToggle={() => setExpanded(!expanded)}
          />
        );
      }
    }
```

(`command` and `expanded`/`setExpanded` are already in scope per the existing tool_call branch.)

- [ ] **Step 3: Type-check + run all TS tests**

Run: `pnpm -s exec tsc --noEmit`
Expected: clean.

Run: `bash scripts/plugin-tests.sh all`
Expected: existing suites still green (the chat-ui test is wired in Task 14).

- [ ] **Step 4: Live smoke (dev)**

Restart dev: `./build.sh kill && ./build.sh dev --wait`. In a chat pane where an agent ran `codeterm send-text …`, confirm the card renders (brand + "Sent to pane X" + ✓ delivered) and expands to the raw command + result.

- [ ] **Step 5: Commit**

```bash
git add packages/chat-ui/src/MessageBubble.tsx
git commit -m "feat(chat): render codeterm CLI commands as branded cards in chat"
```

---

### Task 14: Wire chat-ui test + CI

**Files:**
- Modify: `scripts/plugin-tests.sh`
- Create: `.github/workflows/cli-source-of-truth.yml`

- [ ] **Step 1: Add the chat-ui test to the aggregator**

In `scripts/plugin-tests.sh`, add to the `all)` `tests=(...)` array:

```bash
      packages/chat-ui/src/codeterm/recognize.test.ts
```

- [ ] **Step 2: Run the aggregator**

Run: `bash scripts/plugin-tests.sh all`
Expected: all suites green, including `recognize.test.ts`.

- [ ] **Step 3: Create the CI workflow**

Create `.github/workflows/cli-source-of-truth.yml`:

```yaml
name: cli-source-of-truth

on:
  pull_request:
    paths:
      - 'cli/**'
      - 'docs/cli-manifest.json'
      - 'docs/CLI.md'
      - 'src-tauri/prompts/codeterm_cli.md'
      - 'packages/chat-ui/src/codeterm/**'
      - '.github/workflows/cli-source-of-truth.yml'
  push:
    branches: [develop, master]

jobs:
  cli-doctor:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: cli manifest + doctor tests
        run: cargo test -p codeterm-cli cmd::manifest
      - name: plugin variant coverage
        run: cargo test -p codeterm-cli plugin_parser_handles_each_subcommand

  chat-card-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: '22' }
      - uses: pnpm/action-setup@v4
        with: { version: 10.32.1, run_install: false }
      - run: pnpm install --frozen-lockfile
      - run: npx tsx packages/chat-ui/src/codeterm/recognize.test.ts
```

(Confirm the cli crate's package name with `grep '^name' cli/Cargo.toml`; replace `codeterm-cli` if it differs.)

- [ ] **Step 4: Commit**

```bash
git add scripts/plugin-tests.sh .github/workflows/cli-source-of-truth.yml
git commit -m "ci: run cli source-of-truth doctor + chat-card tests"
```

---

## Final verification

- [ ] `cd cli && cargo test cmd::manifest && cargo test plugin_parser_handles_each_subcommand` — green.
- [ ] `bash scripts/plugin-tests.sh all` — green (incl. `recognize.test.ts`).
- [ ] `pnpm -s exec tsc --noEmit` — clean.
- [ ] `./build.sh check` — clean.
- [ ] Live: agent `codeterm …` calls render as cards; `codeterm cli-manifest --write` produces no diff (artifacts committed).
- [ ] Bump `<!-- revision -->` on any doc touched beyond those already bumped.
