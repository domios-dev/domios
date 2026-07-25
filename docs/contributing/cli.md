# Changing the CLI

The clap tree in `cli/src/args/*.rs` is the single source of truth.

When you add/rename/remove a command or flag:

1. Edit the clap definition in `cli/src/args/`.
2. Run `codeterm cli-manifest --write` — regenerates `docs/cli-manifest.json`,
   the auto-block of `docs/CLI.md`, and the Chat UI command-path tree.
3. If agent-facing behavior changed, update the wording in
   `src-tauri/prompts/codeterm_cli.md` (it is validated, not generated). For
   tab messaging behavior, also check `docs/SKILLS.md`.
4. `cargo test -p codeterm-cli cmd::manifest` must be green
   (`cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, `prompt_cli_commands_exist`).
5. If you added a chat-card template, it is checked by chat-ui `recognize.test.ts`.

CI (`.github/workflows/cli-source-of-truth.yml`) runs all of the above.

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
`config`, `cli-manifest`) are short-circuited early in `cli/src/main.rs` before
socket discovery — but they are still real clap variants, so they appear in the
manifest and docs. Do not reintroduce a separate hand-rolled arg parser.

<!-- revision: 4 -->
