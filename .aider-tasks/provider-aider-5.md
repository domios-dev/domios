# Make the live DEV smoke pass (startup prompt blocks readiness)

Live finding (DEV, your plugin): after `agent spawn aider`, Aider prints at startup
  You can skip this check with --no-gitignore
  Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]:
and waits, so the `>` prompt never appears, boot_stage stays launch_written, and Domios refuses input while the agent is still launching. The owner wants the answer to be Yes (add .aider* to .gitignore).
Fix in the plugin, the same way other providers handle startup dialogs (see how plugins/provider-opencode / packages/plugin-sdk declare classified prompts and safe choices): classify this exact startup question as a known dialog whose safe choice is Yes, so Domios answers it during launch and readiness is reached. Keep `Run shell command?` safe-Yes only for codeterm commands. Add unit tests for the new classification.
The test command runs build, your unit tests, then the live smoke (C:/Users/dev/.local/bin/aider-provider-smoke.sh) which spawns Aider in DEV, waits for recognition + readiness, sends a message through Chat and needs the assistant reply in the tab Chat. Iterate until the whole test command passes. Commit per fix in English.
