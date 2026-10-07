# Task: write a Domios provider plugin for Aider (plugins/provider-aider)

Goal: Aider runs in Domios as a real PTY agent provider, like plugins/provider-opencode. Use these as the exact templates and contract (read-only): plugins/provider-opencode/plugin.json, plugins/provider-opencode/src/plugin.ts, plugins/provider-opencode/src/index.ts, plugins/provider-opencode/package.json, packages/plugin-sdk/src/capabilities.ts.

Create:
- plugins/provider-aider/plugin.json: id "aider", name "Aider", kind provider, transport pty, version same as provider-opencode, the same manifest sections opencode uses (capabilities, behavior_config, models catalogue as free-form so any `openai/<model>` id is accepted). Do not invent manifest keys that opencode's manifest does not use.
- plugins/provider-aider/package.json and src/index.ts mirroring opencode's.
- plugins/provider-aider/src/plugin.ts implementing the same provider functions opencode implements, adapted to Aider:
  - launch: `aider --model <model> --no-auto-commits --no-pretty --no-fancy-input --chat-language English` plus `--config <path>` when the owner set a config path setting; resume is not supported (start fresh in the tab cwd). Never put API keys in argv; they come from the user's Aider config or environment.
  - readiness / idle: Aider is ready and idle when the last non-empty screen line is its input prompt `>` (optionally prefixed by a mode like `ask>` / `architect>`); busy while it streams. Model from the banner line `Model: <name>`. Use structural checks, not long literal sentences.
  - prompts: Aider asks `Run shell command? (Y)es/(N)o` (and `Run shell commands?`). Classify it as a prompt whose SAFE answer is Yes ONLY when every proposed command starts with `codeterm ` (so it can report to the Orchestrator); otherwise it is an owner prompt (not auto-answered). Also classify `Add file to the chat? (Y)es/(N)o` and `Create new file ...? (Y)es/(N)o` as owner prompts.
  - no session files: return null for session/model detection where opencode reads its DB; keep it honest (unknown is null).
- plugins/provider-aider/tests/plugin.test.ts in the style of plugins/provider-opencode/tests/plugin.test.ts covering: launch args (no secrets, config flag), idle/busy detection on sample screens, the shell-command prompt classification (codeterm-only -> safe yes; anything else -> not safe), banner model parse.

Keep every file under 1200 lines, no comments that describe WHAT, English only. Commit with message "feat: Aider provider plugin".
