# Aider task 1: OpenCode config writer (pure, tested)

Create `plugins/provider-opencode/src/opencodeConfig.ts` with pure functions (no host calls, no IO):

- `applyDomiosProviders(config: unknown, providers: DomiosProvider[]): { config: object; changed: boolean }`
  - `DomiosProvider = { id: string; kind: "openai" | "anthropic" | "lmstudio"; baseUrl: string; keyEnv?: string; models?: string[]; name?: string }`
  - Writes each provider into `config.provider[id]` in OpenCode's format:
    - openai / lmstudio: `{ npm: "@ai-sdk/openai-compatible", name, options: { baseURL: baseUrl, apiKey: "{env:KEYENV}" }, models: { [m]: {} } }`
    - anthropic: `{ npm: "@ai-sdk/anthropic", name, options: { baseURL: baseUrl, apiKey: "{env:KEYENV}" }, models: {...} }`
  - Marks every entry it owns with `"x-domios": true` (ownership marker).
  - Removes Domios-owned entries that are no longer in `providers`.
  - NEVER touches entries without the marker (foreign entries stay byte-identical).
  - NEVER writes a literal key: apiKey is only ever `{env:NAME}`; if keyEnv is missing, omit apiKey.
  - `changed` is false when the result is deep-equal to the input.
- `providerKeyEnvName(id: string): string` -> `DOMIOS_PROVIDER_<ID_UPPER_SNAKE>_KEY`.

Tests in `plugins/provider-opencode/tests/opencodeConfig.test.ts` (same style as the existing tests in that folder): add/update/remove owned entries, foreign entry preserved, no literal secret ever (assert no value matches an api-key-looking string), idempotent second apply gives changed=false, env name derivation.

Run `npx tsx plugins/provider-opencode/tests/opencodeConfig.test.ts` and `npx tsc --noEmit` and make them pass. Commit with message `feat(opencode): pure config writer for Domios-owned providers`.

When finished, run this shell command to report (replace the summary):
codeterm send --tab e3ec9ee9 "AIDER task1 done: <commit sha> <tests result one line>"
