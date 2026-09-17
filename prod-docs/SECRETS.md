# Secret store

CodeTerm exposes local secret storage through Settings → Secrets, `codeterm mem secret`, and `/api/secrets/*`. The active backend is configured in `config.toml` and can be changed without restarting.

| Backend | Use | Storage |
|---|---|---|
| `local` | Default, offline | OS keychain in release builds |
| `bitwarden` | Shared Bitwarden or Vaultwarden vault | Plugin over the official `bw` CLI |
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

## Well-known entries

| Name | Read by | Purpose |
|---|---|---|
| `telegram_bot_token` | `POST /api/bridge/start` with `ensure_token` | Telegram bridge bot token |
| `pebble_webhook_secret` | `POST /api/plugins/pebble/webhook` | Bearer secret every inbound Pebble event must present |

Create the Pebble secret from stdin so it never reaches argv or shell history, then read it back once to configure the sender:

```bash
openssl rand -hex 32 | tr -d '\n' | codeterm mem secret save --name pebble_webhook_secret
codeterm mem secret get --name pebble_webhook_secret
```

The daemon reads the entry on every request, so a rotation (`save --overwrite`) applies immediately. A missing, empty, or locked entry makes the route answer `503`; the plugin is never called. Route contract: [API-INTEGRATIONS.md](API-INTEGRATIONS.md#inbound-webhooks-pebble).

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

Avoid `--password`: command-line arguments may be visible to other processes. A no-input `unlock` asks the active backend to restore its own session; it succeeds only for a backend that declares auto-unlock and has already armed it.

- **Lock** drops the in-process session but keeps persisted credentials.
- **Logout** removes the session, any remembered master password, API credentials, and login email.
- Default organization and collection settings apply when `save` omits `--org` or `--collection`.

## The auto-unlock contract

One statement, binding on the CLI, the Settings UI, the `/api/secrets/status` fields, and every
secret-backend plugin.

**Auto-unlock is a capability the backend declares, and a credential the vault authorizes.**

1. **Declaration decides what is offered.** A backend advertises auto-unlock in its manifest:
   `"capabilities": { "secretBackend": { "autoUnlock": true } }`. The bare `"secretBackend": true`
   form declares a store that never restores its own session. Core never infers the capability from
   a backend id, and no surface offers auto-unlock to a backend that did not declare it.
2. **Declaration never authorizes.** A manifest cannot grant vault access. Every secret operation,
   including the unlock that arms auto-unlock, must pass the vault's own authentication on its own
   evidence. A backend that declares the capability and presents no valid credential is refused
   with its own error discriminant.
3. **The backend owns the credential.** Only the backend persists a master password, in its own
   host-provided bucket (`codeterm.plugin.<id>`, the same location `host.secretSet` writes).
   CodeTerm writes no separate credential file. The retired
   `~/.codeterm/secrets/bitwarden-master.env` had no reader; startup overwrites its bytes and
   unlinks it. When the overwrite cannot be proven, startup still unlinks to cut exposure but logs a
   warning that the old secret may remain recoverable and should be rotated — a removed directory
   entry is never reported as a secure erasure.
4. **Persisting requires explicit consent, carried on the wire.** `POST /api/secrets/unlock` takes
   `persist_for_auto_unlock` (default `false`), forwarded to the backend as the JS
   `persistForAutoUnlock` argument. It is always serialized as a bool, so a plugin never has to infer
   intent from an absent field. Only `save-master-password` and the Settings "Unlock and remember"
   form send `true`; ordinary password login and API-key login send `false`. Requesting it from a
   backend that declares no auto-unlock capability is refused with `bad_request`. A backend may keep
   a master password only when the bit is true, and logout must delete it.
5. **Every surface reports the same two facts.** `/api/secrets/status` returns
   `auto_unlock_capable` (the declaration) and `master_password_saved` (declared **and** armed).
   `master_password_saved` is false whenever the backend did not declare the capability, even if
   credential bytes exist somewhere, because a backend that cannot use them has saved nothing.
6. **No surface claims more than it did.** `codeterm mem secret save-master-password` refuses when
   the backend declares nothing, hands the password to the vault when it does, and reports auto-unlock
   as armed only after the status flag says so.

### What a secret-backend plugin must satisfy

- Declare `secretBackend: { autoUnlock: true }` in `plugin.json`, or accept that no surface will
  offer auto-unlock for it.
- Persist a master password only when `secretUnlock` receives `persistForAutoUnlock: true` — not on
  every successful unlock — and store it under `master_password` in the plugin's own bucket. Treat an
  absent or malformed value as `false`.
- Delete that credential in `secretLogout`.
- Keep the credential out of every emitted string: no status field, error message, or log line may
  contain a master password, a session token, or any value derived from one.

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

Bitwarden owns vault encryption. CodeTerm passes the master password and session through stdin or environment variables, never argv. A secret-backend plugin's persisted session and any remembered credential live in its own host bucket, service `codeterm.plugin.<plugin-id>`, in the same keychain abstraction.

A process running as the same OS user remains inside the same trust boundary and may access that user's keychain. The mock backend has no security guarantees.

## Adding a backend

Implementation lives in `src-tauri/src/secrets/`:

1. Implement `SecretStore` and map failures to `SecretError`.
2. Implement `SecretBackendFactory`; keep `build` synchronous and use `init` for async warm-up.
3. Add typed settings to `src-tauri/src/config/schema.rs` and expose them through `provider_config_for`.
4. Register the factory in `secrets::factory::register_builtins`. Settings discovers registered factories automatically.
5. Add deterministic tests using the patterns in `mock.rs`, `local.rs`, and Bitwarden's mocked executor. External binaries and networks do not belong in unit tests.

Override optional organization, collection, sync, or init methods only when the backend supports them. The registry is currently built into the binary; there is no external plugin registration API.

<!-- revision: 9 -->
