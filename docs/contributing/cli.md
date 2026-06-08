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

## Why no hand-parsing

Every command goes through clap so the manifest stays complete. Commands that
must run without the daemon (filesystem-local: `plugin`, `docs`, `system`,
`config`, `cli-manifest`) are short-circuited early in `cli/src/main.rs` before
socket discovery — but they are still real clap variants, so they appear in the
manifest and docs. Do not reintroduce a separate hand-rolled arg parser.
