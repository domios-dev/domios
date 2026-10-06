# Aider as a real Domios provider: readiness in core + secret-safe launch + verified Chat

You are allowed to change the Domios core (Rust, src-tauri/) where a plugin alone cannot fix it.

Measured facts (core's own readiness verdict for your smoke tab): provider_tui=true (your screenHasTui works), terminal_input_ready=false. Cause in core: src-tauri/src/service/launch_input_gate.rs `launch_composer_input_ready_for_frame`:
- ComposerCursorPosition::VisibleAnywhere requires `input.alternate_screen && input.cursor_visible` — Aider is a line-mode REPL, never alternate screen, so this can never be true;
- BottomRegion + structural composer only accepts the prompt glyphs ❯ › → (offline_state_classifier.rs `prompt_glyph_cursor_ready_for_frame`); Aider prints ASCII `>`.
Do:
1. Core: add a capability value `ComposerCursorPosition::PromptLine` (manifest "prompt_line", in src-tauri/crates/codeterm-plugin/src/provider/spawn_strategy.rs) for line-mode REPLs: ready when the cursor is visible and sits on the last non-blank screen row (ComposerInputState has cursor_row and last_nonblank_row), with the plugin's screenHasTui confirming the screen. Wire it in launch_composer_input_ready_for_frame and anywhere else that matches on ComposerCursorPosition (grep it; keep every match exhaustive, no provider-id checks). Add focused unit tests next to the existing ones and run `./build.sh test-one app launch_input_gate`.
2. Plugin manifest: "composerCursorPosition": "prompt_line".
3. Secret: the current plugin puts `export OPENAI_API_KEY=<value>` into the launch command, so the KEY IS TYPED INTO THE TERMINAL (visible on screen and in scrollback) and `export` is not PowerShell. Replace it: the launch command fetches the key at run time inside the shell so the value never appears in the command text: PowerShell `$env:OPENAI_API_KEY=(codeterm mem secret get --name '<name>')`; POSIX `export OPENAI_API_KEY="$(codeterm mem secret get --name '<name>')"` (choose by host.platform() / the shell quoting helper). Unit test: no secret value in the built command.
4. Verify the owner journey yourself on your private DEV instance using C:/Users/dev/.local/bin/aider-dev-toolkit.md (read it): spawn, inspect, read the screen, send through Chat, read `tab chat --json`, read the readiness verdict in the log. Check the parsing is right: exactly one user row for one send, the assistant answer in its own row, order kept, no banner/noise/duplicate rows. Fix what you find.
Commit per fix in English.
