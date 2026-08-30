# Secret store

CodeTerm exposes local secret storage through Settings → Secrets, `codeterm mem secret`, and `/api/secrets/*`. The active backend is configured in `config.toml` and can be changed without restarting.

| Backend | Use | Storage |
|---|---|---|
| `local` | Default, offline | OS keychain in release builds |
| `bitwarden` | Shared Bitwarden or Vaultwarden vault | Official `bw` CLI |
| `mock` | Tests and screenshots only | Memory; erased on restart |

Secrets are local-only. `--mesh PEER mem secret …` is rejected with exit code `7`, and secret routes are never relayed over mesh.

## Quick start

Local storage needs no login:

```bash
echo -n "$VALUE" | codeterm mem secret save --name openai-prod --folder work
codeterm mem secret get --name openai-prod
codeterm mem secret search "openai"
codeterm mem secret list --folder work --limit 20
codeterm mem secret delete --name openai-prod
```

`get` writes the raw value without a trailing newline. Use `--json` for `{item_id, name, value, notes}`. `search` and `list` return metadata only. Saving an existing name requires `--overwrite`; deleting prompts unless `--yes` is supplied.

## Bitwarden

Install `bw`, select Bitwarden in Settings → Secrets, set the server URL before login, then sign in with either a password or API key.

```bash
# macOS
brew install bitwarden-cli

# Linux / cross-platform
npm i -g @bitwarden/cli

# Windows
scoop install bitwarden-cli
```

If `bw` is missing, Settings shows an install action and the CLI exits with code `6`. Custom Vaultwarden servers are supported. Changing the server while logged in requires a new login.

```bash
codeterm mem secret status
codeterm mem secret unlock --email user@example.com  # password from stdin
codeterm mem secret sync
codeterm mem secret lock
codeterm mem secret logout
```

Avoid `--password`: command-line arguments may be visible to other processes. A no-input `unlock` attempts server-side auto-unlock when **Remember master password** is enabled.

- **Lock** drops the in-process session but keeps persisted credentials.
- **Logout** removes the session, remembered master password, API credentials, and login email.
- **Remember master password** is off by default. Enabling it stores the password beside the session token so restart-time auto-unlock works.
- Default organization and collection settings apply when `save` omits `--org` or `--collection`.

## Command reference

```text
status | sync | unlock | lock | logout
save --name NAME [--value VALUE] [--folder FOLDER] [--org ID]
     [--collection ID] [--notes TEXT] [--overwrite]
get --name NAME [--json]
search QUERY
list [--folder FOLDER] [--org ID] [--collection ID] [--limit N]
delete --name NAME [--yes]
```

Exit codes: `0` success, `2` invalid arguments, `3` locked, `4` logged out, `5` not found, `6` unavailable, `7` mesh rejected.

The full REST contract is in [API-STATE.md](API-STATE.md#secrets).

## Storage and security

Release builds use macOS Keychain, Linux libsecret, or Windows Credential Manager after a successful canary probe. Local-secret metadata—never values—is indexed at `~/.codeterm/secrets/local-index.json`.

Dev builds use `<data_dir>/keychain/<service>.json` with Unix mode `0600`; the usual path is `~/.codeterm-dev/keychain/codeterm-secrets.json`. This avoids repeated macOS keychain prompts from changing development binary signatures. It is plaintext and must be treated like an `.env` file.

Production local storage does not silently fall back to plaintext when the OS keychain is unavailable. The backend reports `Unavailable`, and CRUD requests return `503` with the reason.

Bitwarden owns vault encryption. CodeTerm passes the master password and session through stdin or environment variables, never argv. Its persisted session and optional remembered credentials use service `codeterm.secrets.bitwarden` in the same keychain abstraction.

A process running as the same OS user remains inside the same trust boundary and may access that user's keychain. The mock backend has no security guarantees.

## Adding a backend

Implementation lives in `src-tauri/src/secrets/`:

1. Implement `SecretStore` and map failures to `SecretError`.
2. Implement `SecretBackendFactory`; keep `build` synchronous and use `init` for async warm-up.
3. Add typed settings to `src-tauri/src/config/schema.rs` and expose them through `provider_config_for`.
4. Register the factory in `secrets::factory::register_builtins`. Settings discovers registered factories automatically.
5. Add deterministic tests using the patterns in `mock.rs`, `local.rs`, and Bitwarden's mocked executor. External binaries and networks do not belong in unit tests.

Override optional organization, collection, sync, or init methods only when the backend supports them. The registry is currently built into the binary; there is no external plugin registration API.

<!-- revision: 5 -->
