# Fix: provider-aider manifest must pass Domios strict validation

`codeterm plugin validate plugins/provider-aider` (strict daemon parse) fails with:
manifest: parse plugins/provider-aider/plugin.json: missing field `settingsPath` at line 39 column 3
Compare the failing section with the SAME section in plugins/provider-opencode/plugin.json (the reference) and make plugins/provider-aider/plugin.json structurally match it: same keys, required fields present, no invented keys. Keep Aider-specific values. The test command below builds the plugin, runs its tests and runs the strict validation; all three must pass.
Commit message in English: "fix(provider-aider): manifest passes strict validation".
