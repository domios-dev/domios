---
name: cli-contributor
description: "Change CodeTerm CLI commands and audit generated contracts and consumers."
---

# Changing the CLI

The clap tree in `cli/src/args/*.rs` is the single source of truth.

When you add/rename/remove a command or flag:

1. Edit the clap definition in `cli/src/args/`.
2. In canonical, regenerate `generated/cli/cli-manifest.json`, the auto-block of
   `generated/cli/cli.md`, and `packages/chat-ui/src/codeterm/commandPaths.ts` with
   `cli-manifest --write` run by the CLI built from this checkout (`./build.sh cli`
   does both, and also installs into the active dev data directory; see
   `.agents/skills/build/SKILL.md` for regenerating without installing). Never hand-edit these
   three files, and never regenerate them with the installed `codeterm`.
3. If agent-facing behavior changed, update the wording in
   `src-tauri/prompts/codeterm_cli.md` (it is validated, not generated). For
   tab messaging behavior, also check `prod-docs/SKILLS.md`.
4. `./build.sh test-one cli cmd::manifest` must be green (canonical QA).
5. If you added a chat-card template, it is checked by chat-ui `recognize.test.ts`.
6. If the change breaks the CLI↔daemon wire contract (a removed/renamed method
   or required param, a changed field meaning), bump `PROTOCOL_VERSION` in
   **both** `cli/src/protocol.rs` and
   `src-tauri/src/socket/server/protocol.rs` — one value, two embeddings, in
   the same commit. That is what turns a stale binary into the typed
   version-skew error (`-32002`, both versions + recovery command) instead of
   parse noise. Additive changes (new methods, new optional fields) do not
   bump it.

CI (`.github/workflows/cli-source-of-truth.yml`) runs all of the above.

## What the CLI gate does and does not cover

`cmd::manifest` holds the whole gate. Know which test owns what:

- `cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, `chat_ui_command_paths_are_fresh` —
  the three generated artifacts match the live clap tree.
- `shipped_prescriptions_are_runnable` — the strict oracle. Every line a shipped
  surface offers as a runnable `codeterm …` command resolves to a real
  subcommand chain, and every `--long` on it is an option of that leaf or an
  inherited global. Corpus: the embedded `DOCS`, `skills/**`,
  `src-tauri/prompts/**`, `generated/cli/cli.md`, and all clap help text.
- `every_doc_embedded_lives_in_prod_docs_and_nothing_else` — an unregistered
  `prod-docs/*.md` or a registered document outside `prod-docs/` fails.
- `prompt_cli_commands_exist` — subcommand existence only, over
  `codeterm_cli.md` alone. It trims to the longest resolvable prefix, so it does
  **not** validate flags.

**No validator in this repository has any output-shape coverage.** The gate
proves the CLI *accepts* what shipped text prescribes; nothing here asserts what
a command *prints*, its exit code, or its JSON schema. Renaming an output field
or dropping a printed line passes every test above. Cover those yourself.

Two structural blind spots, so you do not mistake green for complete:

- A command named mid-sentence is invisible. The oracle only reads lines whose
  runnable head is `codeterm …`, so ``… shows up in `codeterm mem worker list` ``
  inside a paragraph is never checked.
- Prescriptions stop at `—`, `·`, `//`, `#`, and inside `$(…)`. That is
  deliberate — those open prose or another program's flags — but anything after
  them is unvalidated.

## Historical surfaces are not drift

`docs/release/<version>-*.md` are release records: they describe the CLI as it
stood on their date. Divergence from today's clap tree is expected and is **not**
a defect. Do not rewrite them to match the current tree, and do not add them to
any validation corpus. Design decisions live in plan strides and commits; a
design that is still live belongs in a maintained doc.

## Error & hint hygiene

CLI user-facing lines follow three registers, by severity:

- **Errors** — the operation failed. Lead with `✗ <what went wrong>` and follow
  with a `fix: <copy-pasteable command>` line. Built in
  `cli/src/cmd/plan_errors.rs` (`render_plan_error`). Route fallible plan/task
  RPC calls through `render_plan_call_error(err, &params)` so the structured
  message and the cross-daemon `hint:` (dev-vs-prod daemon mismatch) fire
  uniformly — do not hand-roll `format!("RPC error {code}: {message}")`.
- **Advisory** — the operation succeeded but there's a better/next step. Prefix
  `hint:` (lowercase). Built in `cli/src/cmd/hints.rs`. Never block on these.
- **Loud** — a non-fatal condition the user must not miss. Prefix `WARNING:`.

Keep one register per line; never mix `✗` and `hint:` on the same line.

## Why no hand-parsing

Every command goes through clap so the manifest stays complete. Commands that
must run without the daemon (filesystem-local: `plugin`, `docs`, `system`,
`config`, `cli-manifest`, `essence-map`) are short-circuited early in
`cli/src/main.rs` before socket discovery — but they are still real clap
variants, so they appear in the manifest and docs. The `essence-map` command
reads only `essences/capabilities.md` and `essences/core.md`; its JSONL contract
and source-digest checks are documented in `codeterm docs ESSENCE-MAP`. Do not
reintroduce a separate hand-rolled arg parser.
