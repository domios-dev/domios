# Secret store

Pluggable secret store for CodeTerm. The user and any agent save, retrieve, and search secrets through one CLI surface — `codeterm mem secret …` — and the bytes land wherever the active backend stores them.

> Design doc: `docs/plans/secrets-store.md`. This page is the user-facing reference.

## Backends

| Backend | When to use |
|---|---|
| `bitwarden` (default) | Real, encrypted, end-to-end vault. Shells out to the official `bw` CLI. |
| `mock` | Dev / tests / screenshots. In-memory only, lost on daemon restart. |

Switch in **Settings → Secrets** or in `~/.codeterm/config.toml`:

```toml
[integrations.secrets]
backend = "bitwarden"
default_organization = ""   # Bitwarden org id, empty = personal vault
default_collection = ""     # Bitwarden collection id
```

## Bitwarden setup

1. Install the Bitwarden CLI:
   - **macOS:** `brew install bitwarden-cli`
   - **Linux:** `npm i -g @bitwarden/cli` (or use the Bitwarden APT repo)
   - **Windows:** `scoop install bitwarden-cli`
2. Open **Settings → Secrets** in CodeTerm.
3. Sign in with email + master password, or with a personal API key (Vault → Security → Keys → "View API key").
4. (Optional) Pick a default organization and collection — new items land there unless overridden.

The unlocked session token is persisted in your OS keychain (macOS Security framework, Windows Credential Manager, Linux libsecret) under the service `codeterm.secrets.bitwarden`. **Your master password is never stored.** When the session expires the UI prompts you to re-unlock.

On Linux without libsecret the adapter degrades to in-process storage only and the Settings badge shows `(transient)` — every daemon restart needs a fresh unlock in that mode. We never fall back to plaintext on disk.

## CLI

```bash
# Lifecycle
codeterm mem secret status                     # Backend + auth state
codeterm mem secret unlock --email u@x.com     # Master password from stdin (preferred)
codeterm mem secret lock                       # Drop in-process session
codeterm mem secret logout                     # Sign out + clear OS-keychain creds

# CRUD
echo -n "$VALUE" | codeterm mem secret save --name openai-prod --folder work
codeterm mem secret get --name openai-prod                    # raw value to stdout
codeterm mem secret get --name openai-prod --json             # {name, value, notes}
codeterm mem secret search "openai"
codeterm mem secret list --org ORG_ID --limit 20
codeterm mem secret delete --name openai-prod                 # y/N prompt; --yes skips
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 2 | Bad CLI args |
| 3 | Vault locked — run `codeterm mem secret unlock` |
| 4 | Logged out — sign in via Settings or `codeterm mem secret unlock` |
| 5 | Secret not found |
| 6 | Backend unavailable (e.g. `bw` not installed) |
| 7 | `--mesh` rejected — secrets are local-only by design |

### Etiquette for secrets in agents

- **Always pipe values via stdin** when calling `mem secret save` — `--value` puts the plaintext in shell history.
- **Use `--json` when piping `get` output** to another program. The default raw-value mode is convenient for `mem secret get --name X | pbcopy` but it lands the value in your terminal scrollback if you forget the pipe.
- **Never echo a value to a pane** that's bridged to Telegram, mesh, or any webhook. The CodeTerm sanitizer redacts known field names but it cannot recognize an arbitrary string as "this is a secret."
- **Don't refer to a secret by name in commit messages.** The name itself is searchable metadata; treat names like file paths, not like contents.
- **Prefer per-environment names** (`openai-prod`, `openai-staging`) over reusing one `openai` and switching folders. The CLI lookup is case-insensitive but exact, and ambiguous names error.

## Architecture

The `SecretStore` trait (in `src-tauri/src/secrets/mod.rs`) defines the backend contract: `save`, `get`, `search`, `list`, `delete`, `unlock`, `lock`, `logout`, `status`, plus optional `organizations` / `collections`. REST + Socket + Tauri IPC + CLI all reach the active backend through one `Arc<RwLock<Box<dyn SecretStore>>>` in `ServiceContext.secrets`. Switching backends in Settings swaps the box atomically.

The Bitwarden adapter shells out to `bw` via `tokio::process` with `--nointeraction --response`, so output is always parseable JSON. Master passwords and session tokens travel via env (`BW_PASSWORD`, `BW_SESSION`) or stdin — never on argv (which is visible to other users via `ps`). The `BwExec` trait abstracts the subprocess so unit tests run without a real `bw` binary.

## Mesh boundary

**Secrets do not cross the mesh.**

- The CLI rejects `--mesh PEER mem secret …` with exit code 7.
- The REST router does not relay `/api/secrets/*` over the mesh transport.
- Each peer holds its own vault scoped to its own OS keychain.

A future "secret introduction" protocol could let one peer hand a specific named secret to another behind explicit per-peer consent, but it isn't designed yet — see `docs/plans/secrets-store.md` §Mesh boundary for the open shape.

## Threat model summary

| Leak vector | Mitigation |
|---|---|
| Plaintext on disk | Never written. Only the `bw` session token lands in the OS keychain. |
| Master password in logs | `unlock` handler accepts via JSON body, hands to `bw` via `BW_PASSWORD` env, never logged. `SecretString` has no `Display`/`Debug` of its bytes. |
| `bw` argv visible via `ps` | Master password via env, item value via base64-encoded stdin. Never argv. |
| Value in pane scrollback | Caller's choice. `--json` lets you pipe to `pbcopy` without it landing on screen. |
| Value in mesh / Telegram / webhooks | The `bridge::sanitize` filter drops `value` / `password` / `master_password` fields by name; the dispatch layer refuses to relay secret commands. |
| Other agent on the same UID | The same UID is trusted — same as the rest of CodeTerm. Secrets are no exception. |

Full threat model: `docs/plans/secrets-store.md` §Threat model.

<!-- revision: 1 -->
