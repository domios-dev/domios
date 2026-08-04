# Secret store

Pluggable secret storage. The user and any local agent save, retrieve, and search secrets through one CLI surface (`codeterm mem secret …`) and one REST surface (`/api/secrets/*`); the bytes land wherever the active backend stores them. The trait is small and intentionally stable so a new backend (1Password, Vault, AWS Secrets Manager, …) is one factory + one config struct.

Built-in backends:

| id | Label | What it is |
|---|---|---|
| `local` | Local secret storage | Default. One entry per secret in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager). Offline. |
| `bitwarden` | Bitwarden | Vault accessed via the `bw` CLI. Public `vault.bitwarden.com` or self-hosted Vaultwarden. |
| `mock` | Mock (testing only) | In-memory only. Wiped on daemon restart. Tests and screenshots only. |

Source: `src-tauri/src/secrets/`. Active store is held as `Arc<RwLock<Box<dyn SecretStore>>>` on `ServiceContext.secrets`; switching backends in Settings swaps the box atomically.

## For users

### Choosing a backend

Settings → Secrets has a dropdown with three entries; switching is instant and persists to `config.toml`:

- **Local secret storage** — pick this if you do not already use Bitwarden. Secrets live in your OS keychain under service `codeterm-secrets`. Offline. Nothing leaves the machine. The only metadata file on disk is a non-secret index (names + ids + folders + timestamps) at `~/.codeterm/secrets/local-index.json` — never the values.
- **Bitwarden** — pick this if you want one vault across machines or if your team already runs Vaultwarden. Requires the `bw` CLI; CodeTerm shells out to it.
- **Mock (testing only)** — do not put real secrets here. Used by integration tests and dev screenshots; data evaporates when the daemon restarts.

### Bitwarden

1. Install the `bw` CLI (only needed once on each machine):
   - macOS: `brew install bitwarden-cli`
   - Linux / cross-platform: `npm i -g @bitwarden/cli`
   - Windows: `scoop install bitwarden-cli`
   - If `bw` is not on `PATH`, the Settings page shows backend status `Unavailable` with the install hint and an **Install Bitwarden CLI** button that opens a new Tab and runs the right command for your platform.
2. **Server URL** field (defaults to `https://vault.bitwarden.com`). Point at your Vaultwarden host (`https://vault.example.org`) before signing in. Changing the URL while logged in forces a re-login on the next unlock — `bw` refuses to switch servers with an active session.
3. **Sign in** — two modes:
   - *Password*: email + master password (+ TOTP if enabled).
   - *API key*: client id + client secret from the Bitwarden web vault under Account Settings → Security → Keys → "View API key". You still type the master password once to derive the vault key, but the long-term creds are the API key — useful for headless / CI machines.
4. **Remember master password** toggle — when on, the master password is stored alongside the session token (see [Storage internals](#storage-internals)) so the daemon can auto-unlock after a restart instead of prompting. Off by default. Trade-off: convenient, no surprise prompts; the cost is one more secret on the same disk surface as the session token. Skip it if your threat model does not allow plaintext-on-disk-in-dev.
5. **Default organization / collection** — when set, every saved item without an explicit `--org` / `--collection` lands there. Empty = personal vault.
6. **Sync** — `codeterm mem secret sync` (or restart the daemon) pulls latest from the remote vault. Only Bitwarden does anything here; Local / Mock are no-ops.
7. **Lock** — drops the in-process session token. Persisted creds (session in keychain, optional master password) are kept; next `unlock` reuses them.
8. **Logout** — drops session AND every persisted credential (session, master password, API client id/secret, login email).

### Local

No login. Status is always `Unlocked` once the OS keychain probe succeeds. The status payload reports the current hostname as the user (`HOSTNAME` / `COMPUTERNAME` / `hostname::get`). On Linux without libsecret, the keychain probe fails, the status flips to `Unavailable`, and CRUD calls return `503` with a human reason — we never silently fall back to plaintext on disk in production builds.

### Mock

Status is always `Unlocked` after construction (`unlock` accepts any non-empty password just to exercise the flow). All data lives in a single `Mutex<HashMap>` and is gone on process exit. Use only in dev / tests.

## CLI reference

All subcommands are under `codeterm mem secret`. They talk to `/api/secrets/*` on the local daemon. `--mesh PEER mem secret …` is rejected with exit code `7` — secrets are local-only by design.

```bash
codeterm mem secret status                                # backend + auth state
codeterm mem secret sync                                  # bw sync (Bitwarden only)
codeterm mem secret unlock --email u@x.com                # master password from stdin (preferred)
codeterm mem secret unlock --email u@x.com --password PW  # password on argv (visible via ps; avoid)
codeterm mem secret unlock                                # no input → server-side auto-unlock if persist_master_password is on
codeterm mem secret lock                                  # drop in-process session
codeterm mem secret logout                                # drop session AND persisted creds

# CRUD
echo -n "$VALUE" | codeterm mem secret save --name openai-prod --folder work
codeterm mem secret save --name K --value V --org ORG_ID --collection COLL_ID --notes "..." --overwrite
codeterm mem secret get --name openai-prod                # raw value to stdout, no trailing newline
codeterm mem secret get --name openai-prod --json         # {item_id, name, value, notes}
codeterm mem secret search "openai"                       # positional; metadata-only
codeterm mem secret list --org ORG_ID --folder work --limit 20
codeterm mem secret delete --name openai-prod             # y/N prompt; --yes skips
```

Exit codes: `0` ok, `2` bad CLI args, `3` vault locked, `4` logged out, `5` not found, `6` backend unavailable, `7` `--mesh` rejected.

## Storage internals

The `Keychain` helper (`src-tauri/src/secrets/keychain.rs`) abstracts the keychain so backends never call `keyring::Entry::new` directly. It has three variants:

| Variant | When | Where bytes go |
|---|---|---|
| `Backend::Os` | Release builds with the `secrets-keyring` feature, after a successful canary write/delete probe. | macOS Keychain (`Security.framework`), Linux libsecret (Secret Service), Windows Credential Manager. |
| `Backend::File` | Dev builds (`debug_assertions`), or release builds whose canary probe failed (e.g. headless Linux without libsecret), or `secrets-keyring` feature off. | `<data_dir>/keychain/<service>.json`, mode `0600`, atomic `tmp → rename` writes. Dev path is `~/.codeterm-dev/keychain/<service>.json`. |
| `Backend::InMemory` | Tests / explicitly opted in. | `Mutex<HashMap>`, no persistence. |

Why file storage in dev: every `cargo build` produces a new binary signature, which macOS treats as a new app and prompts on every keychain access — unusable for iterative work. The file backend is plaintext-on-disk gated only by Unix `0600` permissions. Treat the dev keychain file as plaintext.

What gets stored (per service):

| Service | Account | Contents |
|---|---|---|
| `codeterm-secrets` | lowercased secret name | JSON `{value, notes}` for each Local-store secret. |
| `codeterm.secrets.bitwarden` | `session` | Active `bw` session token. |
| `codeterm.secrets.bitwarden` | `master_password` | Master password (only when `persist_master_password = true`). |
| `codeterm.secrets.bitwarden` | `apikey:client_id` | Bitwarden API-key client id (when API-key flow used). |
| `codeterm.secrets.bitwarden` | `apikey:client_secret` | Matching client secret. |
| `codeterm.secrets.bitwarden` | `login_email` | Last successful login email — used by auto-unlock. |

Logout drops every account under `codeterm.secrets.bitwarden`. Local store entries are deleted only when the user explicitly deletes a secret.

## Adding a new backend

A backend is one trait impl + one factory + one config slice. The Settings UI dropdown picks it up automatically from the registry — no UI work.

Skeleton (~30 lines), based on a hypothetical 1Password CLI backend:

```rust
// src-tauri/src/secrets/onepassword.rs
use async_trait::async_trait;
use super::factory::SecretBackendFactory;
use super::{
    SecretStore, SecretValue, SecretRef, SaveRequest, ListFilter,
    StoreStatus, UnlockCredentials, SecretError, validate_name,
};

pub struct OnePasswordFactory;

impl SecretBackendFactory for OnePasswordFactory {
    fn id(&self) -> &'static str { "onepassword" }
    fn label(&self) -> &'static str { "1Password" }
    fn description(&self) -> &'static str {
        "Vault accessed via the 1Password CLI (op)."
    }
    fn build(&self, cfg: &serde_json::Value) -> Box<dyn SecretStore> {
        let parsed: crate::config::schema::OnePasswordSettings =
            serde_json::from_value(cfg.clone()).unwrap_or_default();
        Box::new(OnePasswordStore::detect_and_build(parsed))
    }
}

pub struct OnePasswordStore { /* op_path, session, keychain, ... */ }

#[async_trait]
impl SecretStore for OnePasswordStore {
    fn id(&self) -> &'static str { "onepassword" }
    async fn status(&self) -> StoreStatus { /* probe `op whoami` */ }
    async fn unlock(&self, c: UnlockCredentials) -> Result<(), SecretError> { /* op signin */ }
    async fn lock(&self) -> Result<(), SecretError> { /* drop session */ }
    async fn logout(&self) -> Result<(), SecretError> { /* op signout + clear keychain */ }
    async fn save(&self, r: SaveRequest) -> Result<SecretRef, SecretError> { validate_name(&r.name)?; /* op item create */ }
    async fn get(&self, n: &str) -> Result<SecretValue, SecretError> { /* op item get --reveal */ }
    async fn list(&self, f: ListFilter) -> Result<Vec<SecretRef>, SecretError> { /* op item list */ }
    async fn search(&self, q: &str) -> Result<Vec<SecretRef>, SecretError> { /* op item list + filter */ }
    async fn delete(&self, n: &str) -> Result<(), SecretError> { /* op item delete */ }
    // organizations / collections / sync / init have sensible defaults — override only if relevant.
}
```

Steps:

1. **Implement `SecretStore`** — the eight required methods (`id`, `status`, `unlock`, `lock`, `logout`, `save`, `get`, `list`, `search`, `delete`). Defaults exist for `organizations`, `collections`, `sync`, `init` — override only when meaningful.
2. **Decide what `StoreStatus` means** for your backend:
   - `Unlocked { user, transient, endpoint }` — ready to read/write. `transient = true` means creds are not persisted across restarts (e.g. headless Linux without libsecret).
   - `Locked { endpoint }` — creds known, session sealed. Caller should `unlock` again.
   - `LoggedOut { endpoint }` — backend selected but no creds yet.
   - `Unavailable { reason }` — backend cannot run at all (binary missing, network down, etc.). Settings UI shows the reason verbatim.
3. **Map your backend's errors to `SecretError`** — `LoggedOut`, `Locked`, `NotFound`, `Ambiguous { candidates }`, `AlreadyExists`, `BadRequest { message }`, `Backend { message }`. The HTTP layer already maps these to status codes (`401` / `423` / `404` / `409` / `409` / `400` / `503`); CLI exit codes are derived from those. See `bitwarden::map_bw_message` for a substring-matching example.
4. **Implement `SecretBackendFactory`** — `id`, `label`, `description`, `build(provider_config)`. The factory deserializes its own typed config from the raw `serde_json::Value` it receives; missing or `Null` ⇒ defaults. Keep `build` synchronous and cheap; async warm-up belongs in `SecretStore::init`.
5. **Add the typed config struct** to `src-tauri/src/config/schema.rs`:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, Default)]
   #[serde(default)]
   pub struct OnePasswordSettings { pub account: String, pub vault: String }
   ```
   Add a field to `SecretsConfig` (`pub onepassword: OnePasswordSettings`) and plumb it into `secrets::provider_config_for`:
   ```rust
   map.insert("onepassword".into(), serde_json::to_value(&cfg.onepassword).unwrap_or(Null));
   ```
6. **Register the factory** in `secrets::factory::register_builtins`:
   ```rust
   Box::new(super::onepassword::OnePasswordFactory),
   ```
   The Settings dropdown reads `list_factories()` and renders one option per registered factory, sorted by `label()`.
7. **Tests** — follow the patterns in `mock.rs` (in-memory, no I/O), `local.rs` (`Keychain::in_memory` + `MemIndexStorage`), and `bitwarden.rs` (the `BwExec` trait + `ScriptedExec` mock that pops canned JSON responses off a queue). The canonical pattern is to put your subprocess / network calls behind a small trait so unit tests never need the real binary.
8. **Init / sync if applicable** — if your backend has remote state (anything that benefits from a `sync` button), implement `sync()`. If you can warm up a session at construction (e.g. auto-unlock from persisted creds), implement `init()`; the API layer calls it after every backend swap and on the no-input `unlock` path (`POST /api/secrets/auto-unlock`).

There is intentionally no public registration API for plugins outside the binary today — the registry lives in a `OnceLock` initialized from `register_builtins`. When that changes (loadable adapters), the trait surface above stays the same; the only new step will be discovering the factory.

## Trade-off / threat model

Honest summary.

- **Local + release builds.** Same protection as Chrome / Slack / 1Password CLI. Bytes are encrypted at rest by the OS keychain; require an unlocked login session to read. An attacker with disk access cannot recover values from the index alone — the index never contains secret material.
- **Local + dev builds.** Plaintext on disk gated by Unix `0600`. The keychain file is at `~/.codeterm-dev/keychain/codeterm-secrets.json`. Treat the dev keychain like an `.env` file. We chose this over signed-binary-changes-on-rebuild macOS prompts because the prompts make iterative work impossible; the trade-off is conscious.
- **Bitwarden.** Vault crypto handled by `bw` itself; CodeTerm never sees the plaintext vault on disk. Master password and session token travel via env (`BW_PASSWORD`, `BW_SESSION`) or stdin — never argv. The unlocked session token lives in the keychain (file in dev, OS keychain in release). With `persist_master_password = true`, the master password lives in the same place — opt-in for that exact reason.
- **Mock.** No security claims. Tests only.
- **Mesh.** Secrets do not cross the mesh. The CLI rejects `--mesh PEER mem secret …` with exit code `7`; the dispatch layer refuses to relay `/api/secrets/*` over mesh transport. Each peer holds its own vault scoped to its own keychain.
- **Same-UID adversary.** A process running as your UID can read your OS keychain — that is a same-UID trust boundary, not a CodeTerm one. Same as `git`, `ssh`, and every other CLI on your machine.

<!-- revision: 4 -->
