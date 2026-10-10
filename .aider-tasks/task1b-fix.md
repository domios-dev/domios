Fix plugins/provider-opencode/src/opencodeConfig.ts (keep the API):
1. Never overwrite a FOREIGN entry: if config.provider[id] exists and is NOT Domios-owned (no "x-domios": true), leave it byte-identical and report the id in a new `conflicts: string[]` field of the result (return type becomes { config, changed, conflicts }). Domios-owned entries are still added/updated/removed as before.
2. Remove comments that only describe WHAT the code does; keep at most a one-line WHY comment where truly needed.
3. Add tests in plugins/provider-opencode/tests/opencodeConfig.test.ts: a foreign entry with the same id as a wanted Domios provider stays identical and appears in conflicts; conflicts is empty in the normal case.
Commit message in English: "fix(opencode): never overwrite foreign provider entries".
