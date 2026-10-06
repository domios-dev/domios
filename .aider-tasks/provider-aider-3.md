# Fix: provider-aider manifest must pass Domios strict validation

plugins/provider-aider/plugin.json is in the chat as an editable file. Strict validation fails with:
manifest: parse plugins/provider-aider/plugin.json: missing field `settingsPath` at line 39 column 3
Cause: the "integration" section is OPTIONAL in the manifest schema, but when present it requires settingsPath. Aider has no integration file. Fix: DELETE the whole "integration": { "kind": "none" } section (including its trailing comma handling) from plugins/provider-aider/plugin.json. Do not invent a settingsPath. Do not change anything else.
Commit message in English: "fix(provider-aider): drop the optional integration section".
