# Startup must reach the prompt with the owner's model (live findings)

Live DEV screens show two more startup blockers:
1. `Warning: openai/mimo-v2.6-pro expects these environment variables - OPENAI_API_KEY: Not set` -> the plugin launches bare `aider` without credentials or base URL.
   The owner's working launcher does: key from the Domios secret store (`codeterm mem secret get --name xiaomi-mimo-api-token`), env OPENAI_API_KEY=<key>, OPENAI_API_BASE=https://token-plan-sgp.xiaomimimo.com/v1, and `--config C:/Users/User/.config/aider-mimo/config.yml`.
   Implement plugin settings: `configPath` (adds `--config <path>`), `apiBase` (env OPENAI_API_BASE), `apiKeySecret` (name of a Domios secret; the KEY VALUE goes only into the launch ENVIRONMENT via the provider launch-env mechanism the SDK offers, never argv, never logs, never the plugin settings file). Defaults for this owner may be documented in configHelp but not hard-coded as secrets. Add `--no-show-model-warnings` to the launch.
2. `Open documentation url for more info? (Y)es/(N)o/(D)on't ask again [Yes]:` -> owner decision: answer D immediately. Classify it as a startup dialog with safe choice D (like the .gitignore question with safe Yes).
Keep the gitignore classification, the codeterm-only shell rule and the Chat history projection. Unit tests for: settings -> launch args/env (no secret in argv), both startup dialog classifications. The test command still ends with the live DEV smoke; iterate until it passes. English commits.
