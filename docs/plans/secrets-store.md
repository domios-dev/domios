# Pluggable secret-store layer

**Branch:** `feat/secrets-store` (off `master` at the time of writing)
**Status:** design — Phase 1 not yet started
**Owner:** the secrets feature is parallel to the ongoing release / updater / mesh work; nothing in this plan touches those modules.

## Goal

A unified, backend-agnostic secrets layer inside CodeTerm so that the user and any agent can save, retrieve, and search secrets through the same `codeterm mem secret …` surface — regardless of where the bytes actually live.

Default backend at ship: **Bitwarden**, via the official `bw` CLI (we never reimplement the crypto). The abstraction is shaped so 1Password, KeePassXC, macOS Keychain, gopass, etc. can drop in later behind the same trait.

## Non-goals

- Reimplementing Bitwarden, 1Password, or any vault crypto.
- Cross-mesh secret reads. A peer's General Agent **cannot** read this machine's vault. Any `--mesh` argument on the new commands is rejected with a clear error in Phase 1; a future `mesh-trust` design lives outside this doc.
- Replacing the existing `~/.codeterm/secrets/telegram.token` file in this iteration. That stays as-is; migrating it onto the new layer is a follow-up once the layer is proven.
- Sharing one vault across multiple OS users on the same machine. Each user has their own keychain entry.

---

## 1. Trait + lifecycle

```rust
// src-tauri/src/secrets/mod.rs

#[async_trait::async_trait]
pub trait SecretStore: Send + Sync {
    /// Stable backend id ("bitwarden", "mock", "keychain", …).
    fn id(&self) -> &'static str;

    /// Current store status — drives the Settings UI badge.
    async fn status(&self) -> StoreStatus;

    /// Try to unlock with provided credentials. Concrete shape depends on backend.
    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError>;

    /// Forget the in-process session (vault stays on disk).
    async fn lock(&self) -> Result<(), SecretError>;

    /// Drop credentials entirely (clears keyring entry too).
    async fn logout(&self) -> Result<(), SecretError>;

    /// Create or update a secret. Returns backend-stable item id.
    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError>;

    /// Resolve a secret value (the only call that returns plaintext).
    async fn get(&self, item_id: &str) -> Result<SecretValue, SecretError>;

    /// Listing + search return metadata only — never the value.
    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError>;
    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError>;

    /// Hard delete (Bitwarden: --permanent so it doesn't sit in trash).
    async fn delete(&self, item_id: &str) -> Result<(), SecretError>;

    /// Optional — list orgs/collections for the Settings picker.
    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> { Ok(vec![]) }
    async fn collections(&self, org_id: Option<&str>) -> Result<Vec<CollectionRef>, SecretError> { Ok(vec![]) }
}

#[derive(Debug, Clone, Serialize)]
pub enum StoreStatus {
    NotConfigured,           // backend not selected yet
    LoggedOut,               // configured but no creds
    Locked,                  // creds present, session sealed
    Unlocked { user: String }, // ready to read/write
    Unavailable { reason: String }, // bw not on PATH, keyring crate failed, etc.
}

pub struct UnlockCredentials {
    pub master_password: SecretString,            // never logged
    pub two_factor_token: Option<SecretString>,
}

pub struct SaveRequest {
    pub name: String,
    pub value: SecretString,                      // zeroed on drop
    pub notes: Option<String>,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    pub overwrite: bool,                          // false = error if name exists
}

pub struct SecretRef {
    pub item_id: String,                          // backend-stable id
    pub name: String,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    pub last_modified: Option<i64>,
}

pub struct SecretValue {
    pub item_id: String,
    pub name: String,
    pub value: SecretString,                      // zeroed on drop
    pub notes: Option<String>,
}
```

`SecretString` is the [`secrecy`](https://crates.io/crates/secrecy) crate's wrapper — it zeroes on drop, has no `Display`/`Debug` for the secret, and only `expose_secret()` reveals the bytes. Every code path that handles values uses it; nothing else can accidentally print or log them.

### State plumbing

A single `SecretStoreState = Arc<RwLock<Box<dyn SecretStore>>>` is added to `ServiceContext` (`src-tauri/src/service/mod.rs`) right alongside `mem`, `agent_tracker`, etc., and surfaced in `AppState` (`src-tauri/src/api/routes.rs`) so REST + Socket + Tauri IPC all reach it through the same handle.

On daemon start:
1. Read `integrations.secrets.backend` from `config.toml` (default `"bitwarden"`).
2. Construct the matching adapter (`Box<dyn SecretStore>`).
3. The adapter restores its session lazily on first call (no blocking startup if Bitwarden is down).

When the user changes the backend in Settings:
1. `update_config(...)` writes the new backend id.
2. A `secrets-backend-changed` event fires.
3. The daemon swaps `*state.write().await = build_adapter(new_id)?;` atomically.
4. Frontend re-queries `status()` and re-renders the Secrets section.

### Reload semantics

Only the active backend lives in memory at a time. If the user switches `bitwarden` → `mock`, the previous Bitwarden session is dropped (next call would have to unlock again anyway). Two backends are never instantiated simultaneously.

---

## 2. Bitwarden adapter

`src-tauri/src/secrets/bitwarden.rs`. The adapter never re-implements vault crypto. It uses two integration paths:

- **Hot path: `bw serve`** — we spawn `bw serve --port <ephemeral> --hostname 127.0.0.1` once at adapter construction and hold the child handle plus a `reqwest::Client`. Cold-start of `bw` is ~500 ms; `bw serve` reduces every subsequent CRUD call to a localhost REST hop and keeps `BW_SESSION` custody inside the single child process — it never crosses argv or env boundaries again. `bw serve` rejects requests with an `Origin` header by default (we don't send one).
- **Cold path: direct `bw <cmd>`** — used only for `login`, `logout`, and the *initial* `unlock` (because `bw serve` requires `BW_SESSION` to already be set in its env, which means we need to unlock once via the CLI to bootstrap). Also used for `bw encode` if we ever bypass the serve JSON body mode (we don't, in v1).

Every CLI invocation passes `--nointeraction --response` so:
- We don't deadlock waiting on a tty prompt that will never come.
- We get `{"success":bool,"data":…,"message":"…","errorCode":…}` JSON on stdout for **both** success and failure (instead of human prose on stderr). Exit codes are coarse (`0` / `1`) so we always read the JSON.

### Detection

On adapter construction:
- Look up `bw` on `PATH` (`which::which("bw")`).
- If missing, the adapter still constructs, but `status()` returns `Unavailable { reason: "bw not installed" }` and Settings shows install instructions per-platform (`brew install bitwarden-cli`, `npm i -g @bitwarden/cli`, `scoop install bitwarden-cli`, `apt install bitwarden-cli` from the Bitwarden APT repo).
- If found, we don't start `bw serve` until the first call that needs it (lazy startup so daemon boot stays fast even when Bitwarden is down).

### Auth flow

Two supported entry points:

1. **Interactive login** (recommended for desktop): user types email + master password in the Settings panel; daemon runs:
   ```
   bw login <email> <password> [--method <2fa>] [--code <token>]
   ```
   then `bw unlock --raw --passwordenv BW_PASSWORD` to get a session token on stdout.

2. **API key login** (for headless / CI / cron): user provides Bitwarden client_id + client_secret (created in their account web vault). Daemon runs:
   ```
   BW_CLIENTID=… BW_CLIENTSECRET=… bw login --apikey
   ```
   followed by `bw unlock --raw --passwordenv BW_PASSWORD` for the master password. The API key authenticates the device; the master password is still required to unlock the vault.

The captured session token is stored two places:
- In-process: `Arc<RwLock<Option<SecretString>>>` on the adapter; passed via `BW_SESSION` env to every subsequent `bw` invocation.
- At rest: OS keychain via the `keyring` crate (see §4). On daemon restart we attempt to read it back so the user doesn't have to log in again.

`bw status --session $BW_SESSION` is parsed (JSON → `{ status: "unauthenticated" | "locked" | "unlocked", userEmail, … }`) to populate `StoreStatus`.

### Item shape

A CodeTerm "secret" maps to a Bitwarden Login item (`type: 1`):
- Bitwarden item `name` ← `SaveRequest.name`
- `login.password` ← `SaveRequest.value`
- `login.username` left empty (we're storing opaque secrets, not credentials)
- `notes` ← `SaveRequest.notes`
- `folderId` resolved by looking up `SaveRequest.folder` against `GET /list/object/folders` (created on demand via `POST /object/folder`)
- `organizationId` / `collectionIds` populated when the user has chosen a default org/collection in Settings.

The `bw serve` REST mappings used:

| Operation | `bw serve` route | Notes |
|---|---|---|
| save (create) | `POST /object/item` | Body is raw JSON (serve handles encoding internally — no `bw encode` step needed). |
| save (update) | `PUT /object/item/<id>` | Same body shape. |
| get (metadata) | `GET /object/item/<id-or-name>` | Returns full cipher; we extract `login.password` for the value path. |
| get (value-only) | `GET /object/password/<id-or-name>` | Returns just the password string in `data`. |
| delete | `DELETE /object/item/<id>?permanent=true` | We always pass `permanent` — no trash semantics for our use case. |
| list | `GET /list/object/items?search=…&organizationid=…&collectionid=…&folderid=…` | `folderid=null` literal filters un-foldered items. |
| list orgs | `GET /list/object/organizations` | |
| list collections | `GET /list/object/collections?organizationid=…` | |
| sync | `POST /sync` | Called after every save/delete and before list when local snapshot >60 s old. |
| status | `GET /status` | Returns `{status: "unauthenticated" | "locked" | "unlocked", userEmail, …}`. We poll this on every adapter call to choose the recovery path. |

`bw get item` / `bw get password` will exit 1 and return `{"success":false,"message":"More than one result was found …"}` if a name matches multiple ciphers. The adapter handles this by always passing the cipher `id` (not the name) once the cipher has been observed once and cached in the mem layer.

### Error mapping

The `bw serve` JSON wrapper makes this clean — `success: false` with one of these `message` substrings:

| `message` substring | Mapped to |
|---|---|
| `bw` not on PATH | `StoreStatus::Unavailable` + actionable install hint |
| `Vault is locked.` | `SecretError::Locked` — UI prompts re-unlock; serve `POST /unlock` recovers |
| `You are not logged in.` | `SecretError::LoggedOut` — serve cannot recover; need direct `bw login` |
| `Not found.` | `SecretError::NotFound` |
| `More than one result was found` | `SecretError::Ambiguous(Vec<id>)` — UI shows picker |
| `ECONNREFUSED` / `ETIMEDOUT` / `getaddrinfo` (in JSON message or reqwest error) | `SecretError::Backend(String)` with retry hint |
| Anything else | `SecretError::Backend(message)` |

If the serve subprocess dies (PID gone, port stops responding), the adapter respawns it transparently on the next call and counts respawns; >3 in 60 s flips status to `Unavailable` with the last error so the user sees something explicit.

### Session lifetime + recovery

Session tokens have no documented TTL but expire eventually. Detection works in two layers:
1. The `bw serve` REST response will return `{"success":false,"message":"Vault is locked."}` once the underlying token expires. Adapter calls `POST /unlock` with the master password from a one-shot in-memory cache (cleared after the call) to silently recover. If the cache is empty, status flips to `Locked` and the Settings UI / CLI prompts the user.
2. Periodic `GET /status` (every 5 minutes) updates the in-process snapshot used by `status()` so the UI badge reflects reality without waiting for the next user action.

The CLI surfaces the locked state as exit code `3` + `{"error":"vault locked, run codeterm mem secret unlock"}`.

---

## 3. mem CLI surface

The new commands live under `codeterm mem secret …`, mirroring the rest of the mem surface. `MemAction` (cli/src/main.rs:388) gains:

```rust
#[clap(subcommand)]
Secret(MemSecretAction),

#[derive(Subcommand)]
pub enum MemSecretAction {
    /// Save a secret. Value comes from --value or stdin (preferred).
    Save {
        #[arg(long)] name: String,
        #[arg(long)] value: Option<String>,        // omit + pipe stdin = preferred
        #[arg(long)] folder: Option<String>,
        #[arg(long)] org: Option<String>,
        #[arg(long)] collection: Option<String>,
        #[arg(long)] notes: Option<String>,
        #[arg(long)] overwrite: bool,
    },
    /// Resolve and print a secret. Default = stdout, no trailing newline.
    Get {
        #[arg(long)] name: String,
        /// Print as JSON {"name","value","notes",…} instead of raw value.
        #[arg(long)] json: bool,
    },
    /// FTS5 + vector search over saved secrets (metadata only).
    Search { query: String },
    /// List saved secrets (metadata only).
    List {
        #[arg(long)] org: Option<String>,
        #[arg(long)] folder: Option<String>,
    },
    /// Delete by name. Permanent — no trash.
    Delete {
        #[arg(long)] name: String,
        /// Skip confirmation (defaults to interactive y/N when stdin is a tty).
        #[arg(long)] yes: bool,
    },
    /// Show backend + auth status.
    Status,
    /// Re-pull the backend index into the local mem cache.
    Sync,
}
```

### JSON output

Every command supports the existing top-level `--json` flag. Default JSON shapes:

```jsonc
// status
{"backend":"bitwarden","status":"unlocked","user":"andrey@example.com","org":"Personal"}

// list / search
[{"name":"openai-prod","folder":"work","org":"QuantumSoft","item_id":"…","last_modified":1714712345}]

// get --json
{"name":"openai-prod","value":"sk-…","notes":null}

// get (no --json)  →  raw value to stdout, no trailing newline, exit 0

// save / delete
{"ok":true,"name":"openai-prod","item_id":"…"}
```

### Exit codes

- `0` — success
- `2` — bad CLI args (clap default)
- `3` — store locked, user must unlock (CLI or Settings)
- `4` — store not configured / logged out
- `5` — secret not found
- `6` — backend unavailable (e.g. bw missing, network down)
- `7` — explicit `--mesh` rejected ("secrets do not cross the mesh")

### REST shape

Mirrors the CLI through new routes under `/api/secrets/…` (auth via the existing `auth_middleware`):

```
GET    /api/secrets/status
POST   /api/secrets/unlock      { master_password, two_factor? }
POST   /api/secrets/lock
POST   /api/secrets/logout
GET    /api/secrets/items?folder=&org=&q=
GET    /api/secrets/items/{name}                     // metadata
POST   /api/secrets/items/{name}/value               // resolve value (POST so it never lands in URL logs)
POST   /api/secrets/items                            // save
DELETE /api/secrets/items/{name}
POST   /api/secrets/sync
GET    /api/secrets/organizations
GET    /api/secrets/collections?org=
```

`POST /api/secrets/items/{name}/value` rather than `GET` because (a) we want zero chance of the value showing in URL access logs, (b) it lets us require an explicit body acknowledging the reveal in the future without breaking clients.

### Mem-cache model

Saved secrets *also* land in `mem_items` as `kind=secret`, with the body holding only a JSON pointer:

```json
{
  "backend": "bitwarden",
  "item_id": "1c7f…",
  "org": "QuantumSoft",
  "folder": "work",
  "cached_meta": { "last_modified": 1714712345 }
}
```

That gives us `mem search` for free over secret names/folders (FTS5 + vector), without ever touching the cloud. `codeterm mem secret get` is then:

1. Look up the mem_item by `name` (exact match → fall through to fuzzy via `mem search` if requested).
2. Pull `item_id` out of the body.
3. Call `SecretStore.get(item_id)`.

`codeterm mem secret sync` reconciles the two sides: pull `bw list items`, upsert mem_items, mark items deleted upstream as `tags=["orphan"]` rather than hard-delete (so `mem history` keeps a paper trail).

---

## 4. OS-keychain persistence

The `keyring` crate (https://crates.io/crates/keyring) gives us a single API over:

- macOS — Security framework (`SecKeychain`)
- Windows — Credential Manager
- Linux — libsecret (Secret Service API; works with GNOME Keyring, KWallet, KeePassXC's secret service shim)

What we store there:

| Keyring service | Account | Value |
|---|---|---|
| `codeterm.secrets.bitwarden` | `session` | bw session token (raw string from `bw unlock --raw`) |
| `codeterm.secrets.bitwarden` | `apikey:client_id` | Bitwarden API key client_id (when API key flow is used) |
| `codeterm.secrets.bitwarden` | `apikey:client_secret` | Bitwarden API key client_secret |

**That is the only on-disk secret material the layer creates.** No master passwords, no plaintext tokens in `config.toml`, no fallback file under `~/.codeterm/`.

### Linux portability gotcha

`keyring` on Linux requires libsecret + a running Secret Service daemon. On a headless Linux box (CI, server install) that won't be present. Behavior:

1. Probe at adapter construction time (`keyring::Entry::new(...).set_password("__probe__", "")` and immediately delete).
2. On failure, the Bitwarden adapter degrades to **no persistence**: every daemon restart needs a fresh unlock. `status()` returns `StoreStatus::Unlocked { … }` only for the lifetime of the process; we add `transient: true` to the JSON so the UI can warn.

**We never fall back to plaintext on disk.** If the keyring is unavailable AND the user wants persistence, they can use the API-key login flow with `BW_CLIENTID`/`BW_CLIENTSECRET` set as systemd unit env vars (their responsibility to lock down).

### Feature flags

- `secrets-keyring` — enabled by default on macOS, Windows, and Linux. Pulls in the `keyring` crate.
- Without the feature, the persistence layer is a no-op and the adapter behaves exactly like the headless-Linux case above.

---

## 5. Settings UI wireframe

Settings → Secrets is a new section added to `SettingsPanel.tsx` (slot it between **Integrations** and **Mesh**). The frontend agent will polish the styling; the wireframe below specifies behavior.

```
┌─ Secrets ─────────────────────────────────────────────────────────────┐
│                                                                       │
│  Backend                                                              │
│  ╔════════════════════╗                                              │
│  ║ Bitwarden         ▾ ║  ← dropdown: Bitwarden | Mock (dev only)    │
│  ╚════════════════════╝                                              │
│                                                                       │
│  Status: ● Unlocked as andrey@example.com                            │
│         (or)                                                          │
│  Status: ○ Locked     [ Unlock ]                                     │
│  Status: ○ Logged out [ Log in ▾ Password | API key ]                │
│  Status: ⚠ Bitwarden CLI not installed                                │
│           › macOS:   brew install bitwarden-cli                       │
│           › Linux:   ...                                              │
│           › Windows: scoop install bitwarden-cli                      │
│                                                                       │
│  ── Login (when Logged out) ──                                       │
│  Email           [_________________________]                          │
│  Master password [•••••••••••••••••••••••••]    ← never persisted    │
│  Two-factor      [______]   (TOTP / Yubikey / …)                      │
│  [ Sign in ]                                                          │
│                                                                       │
│  ── API key (alternative tab) ──                                     │
│  Client ID       [_____________________________]                      │
│  Client secret   [•••••••••••••••••••••••••••]                       │
│  Master password [•••••••••••••••••••••••••••]                       │
│  [ Sign in with API key ]                                             │
│                                                                       │
│  ── Default scope (when Unlocked) ──                                 │
│  Organization    [ Personal ▾ ]                                       │
│  Collection      [ (none)   ▾ ]    ← writes default to here          │
│                                                                       │
│  [ Lock ]   [ Sign out ]                                              │
│                                                                       │
│  ── Recently saved (last 5) ────────────────────────                  │
│  • openai-prod      work / QuantumSoft   2 min ago                    │
│  • slack-webhook    home                 1 day ago                    │
│  …                                                                    │
└───────────────────────────────────────────────────────────────────────┘
```

UI rules:

- The master password **never** leaves the React component beyond the `POST /api/secrets/unlock` call. It's not put into Zustand state, never logged, the input field is `type="password"` with `autocomplete="current-password"`.
- After successful unlock the form clears. The form has no "remember password" checkbox — persistence is the session token stored in the OS keychain, not the password.
- The **Recently saved** list calls `GET /api/secrets/items?limit=5&order=last_modified` (metadata only; no values).
- "Sign out" calls `/api/secrets/logout`, which `bw logout`s and deletes the keyring entry.
- Backend `Mock` is only visible when `CODETERM_DEV=1` or in dev builds — it's the in-memory store used for tests and screenshots.

---

## 6. Threat model

| Leak vector | Mitigation |
|---|---|
| Plaintext on disk | We never write secret material to disk. Only the bw session token in OS keychain. `config.toml` holds backend choice + default org, nothing sensitive. |
| Master password in logs | The `unlock` REST handler accepts the password via JSON body, hands it to `bw login`/`bw unlock` via `--passwordenv BW_PASSWORD` (env, not argv) and zeroes the buffer. The handler `log::info!` line never includes the body. We add a regex test that `tracing::field` macros for `unlock` redact the password. |
| Master password in panic backtrace | `SecretString` from the `secrecy` crate has no `Display`/`Debug` impl — panic messages can't accidentally render the bytes. |
| Secret value in `codeterm get-text` scrollback | When a user `mem secret get foo` prints the value to a CodeTerm pane, the value lives in xterm.js scrollback like any other terminal output — that's the user's choice. We document this and offer `--json` so they can pipe to `pbcopy` without the value ever appearing on screen. |
| Secret value in mesh announcements | Mesh status messages use the existing `bridge::sanitize` filter — we extend it to drop any `value` / `password` / `master_password` field by name regardless of nesting. Plus: the secret-store API itself rejects `--mesh`. |
| Secret value bridged to Telegram | The Telegram bridge uses the same `sanitize` function. The dispatch layer also refuses to relay `Command::SecretGet` over the mesh transport (only local origins allowed). |
| Secret value shown in `pane.send_text` history | Same: the dispatch logger redacts payload for `Command::SecretSave` and the `value` field on `Command::SecretGet` responses. |
| Other agent on the box reads `~/.config/Bitwarden CLI/data.json` | Out of scope — that's the upstream Bitwarden vault file with the user's encrypted blob, protected by the master password. We don't touch it. |
| Secret leaking via `[Report from …]` agent reports | Agent reports go through the same sanitizer. |
| `bw` argv visible to other users via `ps` | Hot path uses `bw serve`'s localhost REST — values go in HTTP request bodies, never on argv. The cold path (`bw login`, `bw unlock`) passes the master password via `--passwordenv BW_PASSWORD`, never on argv. The `BW_SESSION` env is set on the spawned `bw serve` child only and never leaks to other processes. |
| `bw serve` local REST exposed to other local users | Bound to `127.0.0.1` on an ephemeral port. `bw serve` rejects requests with an `Origin` header by default (we never send one). Port number is held in-process and never written to disk. Other local users on the same machine could in theory hit the port — a future hardening step is to add a per-instance HMAC header gate, but for v1 the trust model matches the rest of CodeTerm: same-UID is trusted. |

A repo-wide `tracing` filter prevents the `secrets::*` modules from logging at `debug` or higher in release builds.

---

## 7. Test plan

### Unit (Phase 1)

- `mock::InMemoryStore` implementing `SecretStore` with deterministic ids. Lives in `secrets/mock.rs` behind `#[cfg(test)]` plus a feature flag so the dev-builds can expose it as a real backend.
- Trait conformance suite: a single `async fn run_store_suite<S: SecretStore>(store: S)` that exercises save → get → list → search → delete → re-save → status transitions. Both `MockStore` and (optionally, gated) `BitwardenStore` run through it.
- CLI parsing tests for `MemSecretAction` clap definitions (good args, missing required, conflicting flags).
- Sanitizer tests: pump a faked `Command::SecretGet` response through the dispatch logger and assert the value never appears.
- Mem-cache tests: save → mem_item exists with `kind=secret`, body has no value field; delete → mem_item gone.

### Integration (Phase 2)

- `BitwardenStore` end-to-end test gated behind `CODETERM_BW_INTEGRATION=1` env var (and ignored otherwise), so CI without a real vault stays green. Test reads `BW_TEST_EMAIL` / `BW_TEST_MASTER_PASSWORD` from env, expects a sandbox Bitwarden account, and runs the full conformance suite. Documented in `docs/TESTING.md`.
- Locally, the integration test creates and tears down items prefixed `codeterm-test-<random>-` so failure modes don't litter the user's vault.
- A `bw stub` script (Bash + a tiny PowerShell version) lives under `tests/fixtures/bw_stub/` and is `PATH`-prepended in unit tests of `BitwardenStore` to verify argv shaping, env propagation, and exit-code mapping without needing a real `bw` binary. The stub also serves a fake `bw serve` HTTP surface on the requested port (small Python http.server in a sidecar) so the reqwest path can be exercised end-to-end against canned responses (success, vault-locked, ambiguous match, network failure).

### UI (Phase 3)

- Playwright/CDP smoke: open Settings → Secrets, switch backend dropdown to Mock, sign in (mock accepts any creds), save a secret via the form, list shows it, delete clears it.
- Manual test plan in `docs/SECRETS.md` covers: real Bitwarden login (interactive), API-key login, lock/unlock cycle, sign-out, backend swap.

### Regression (Phase 4)

- `prompt-evaluator` doesn't apply (no prompt files changed).
- `code-reviewer` runs on the full Phase-1+2 diff before merge.

---

## 8. Phasing

| Phase | Scope | Branch | Deliverable |
|---|---|---|---|
| 1 | Trait, `SecretError`, `SecretString`, `MockStore`, dispatch wiring, REST routes, `codeterm mem secret …` CLI, unit tests | `feat/secrets-store` | App still works; mem secret commands operate against in-memory mock; `codeterm mem secret status` returns `mock`. |
| 2 | `BitwardenStore`, OS-keychain persistence (`keyring` crate, feature-flagged), conformance suite, integration test gated by env flag | `feat/secrets-bitwarden` (off Phase 1) | Real Bitwarden works end-to-end from the CLI on macOS / Windows / Linux desktop. |
| 3 | Settings → Secrets section in `SettingsPanel.tsx`, backend picker, login forms (password + API key), org/collection picker, recently-saved list | `feat/secrets-settings-ui` | The user can configure Bitwarden entirely from the GUI. |
| 4 | `docs/SECRETS.md`, `docs/API.md` Secrets section, mem skill update, revision tags bumped | `docs/secrets` (or folded into Phase 3 PR) | Docs and skill point at the new commands. |

Each phase compiles, has its own tests, and gets a `code-reviewer` pass before merge.

## 9. Open questions

1. **Mem schema:** keep secrets inside `mem_items` with `kind=secret`, or split them into a dedicated `secret_items` table? The "one table, kind discriminator" approach is simpler and gets us free FTS5 + vector search, but it does mean a `mem_items` row with `kind=secret` will be embedded into the same vector index alongside notes and skills. Lean toward staying in `mem_items`, but worth a check before Phase 1 lands.
2. **Per-pane vs per-machine auth:** all panes inside the same daemon share the unlocked vault. That's the right default for a desktop app. If we ever want per-pane scoping (e.g. "agent X in pane Y can only read secrets tagged `work`"), it's a layer on top — not in the trait surface for now.
3. **Naming:** `codeterm mem secret` reads as "this is a memory subcommand," which it isn't really — secrets are stored externally. We could surface a top-level `codeterm secret` aliasing the same handlers. The user's brief says reuse the mem surface; sticking with that for v1, will revisit if it's noisy in practice.

<!-- revision: 2 -->
