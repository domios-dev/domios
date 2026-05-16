# Domain Review: Config & Secrets

## Files Reviewed

- `src-tauri/src/config/mod.rs` (476 lines)
- `src-tauri/src/config/schema/mod.rs` (751 lines)
- `src-tauri/src/config/schema/tests.rs` (60 lines)
- `src-tauri/src/secrets/mod.rs` (563 lines)
- `src-tauri/src/secrets/api.rs` (534 lines)
- `src-tauri/src/secrets/local.rs` (715 lines)
- `src-tauri/src/secrets/mock.rs` (571 lines)
- `src-tauri/src/secrets/factory.rs` (93 lines)
- `src-tauri/src/secrets/keychain.rs` (335 lines)
- `src-tauri/src/secrets/bitwarden/mod.rs` (80 lines)
- `src-tauri/src/secrets/bitwarden/exec.rs` (87 lines)
- `src-tauri/src/secrets/bitwarden/store.rs` (279 lines)
- `src-tauri/src/secrets/bitwarden/secret_store.rs` (463 lines)
- `src-tauri/src/secrets/bitwarden/cipher.rs` (153 lines)
- `src-tauri/src/secrets/bitwarden/subprocess.rs` (181 lines)
- `src-tauri/src/secrets/bitwarden/tests.rs` (655 lines)

---

## Findings

### src-tauri/src/secrets/bitwarden/subprocess.rs

#### HIGH

- **`subprocess.rs:166-172` — Session token logged in plaintext on `bw` JSON parse failure.**
  When `bw unlock --raw --nointeraction --response` is called, some older `bw` versions emit the raw session token as a bare string instead of a JSON envelope. When `serde_json::from_str::<BwResponse>(stdout.trim())` fails, the fallback at line 166 logs `stdout.trim()` verbatim via `log::warn!`. If that `stdout` contains the raw session token (e.g. from a `bw` version where `--raw` overrides the `--response` wrapper), the session token appears in the log file in plaintext. The same value is also passed into `BwResponse::message`, which can propagate further into error messages emitted by callers. Mitigation: scrub or replace `stdout` with a fixed sentinel in the fallback branch instead of emitting the raw bytes.

#### MEDIUM

- **`subprocess.rs:167-171` — `stderr` also logged verbatim in the unparseable-output path.** While `stderr` is less likely to contain secret material, `bw` has historically echoed back authentication tokens in error output for certain failure modes. The same scrub that fixes the `stdout` finding should be applied to `stderr` in this branch.

---

### src-tauri/src/secrets/bitwarden/secret_store.rs

#### HIGH

- **`secret_store.rs:159-163` — Two-factor authentication code passed in `argv`, visible via `ps`.**
  The TOTP/2FA code is pushed directly into the `args` vec (`args.push(tok.expose_secret())`), which means it appears in the process argument list visible to other OS users via `ps aux` or `/proc/<pid>/cmdline` for the duration of the `bw login` subprocess. The master password is correctly routed through `BW_PASSWORD` env var, but the `--code` argument has no equivalent env var in the `bw` CLI. The only mitigation available is to write the code to a temp file and use `--code "$(cat file)"` via stdin, or to accept that TOTP codes are time-limited and the window is very short. At minimum this should be documented as a known trade-off rather than silently treated as safe.

- **`secret_store.rs:325` — Name validation silently discarded in Bitwarden `get()`.**
  `validate_name(name_or_id).ok()` throws away the `Err` from validation. This means an empty name, a name containing NUL bytes, or a name exceeding 128 chars is passed directly to `bw get item <name>` as a subprocess argument. For `save()` (line 275) the error is correctly propagated. The inconsistency means the `get` path has a weaker invariant and NUL injection into a subprocess argument is not blocked. Fix: propagate the error with `?` identical to `save`.

#### MEDIUM

- **`secret_store.rs:283-284` — `find_by_name` called before `save()` validates availability.**
  `save()` does not call `require_available()` first (unlike `LocalStore::save()`). The first network call (`find_by_name`) would fail with a session error if the store is unavailable, which is a worse error message than the explicit `SecretError::Backend { message: unavailable_reason }` that `require_available()` would return. Low impact but inconsistent with every other operation in the file.

---

### src-tauri/src/secrets/bitwarden/cipher.rs

#### LOW

- **`cipher.rs:104` — `unwrap()` on a `Vec::into_iter().next()` that is guarded by `len == 1`.**
  The guard on line 102 (`1 =>`) makes this logically infallible, but the compiler cannot prove it. An `expect("len==1 proven by guard")` communicates intent; panics in production would only fire if the guard logic were broken by a future edit.

---

### src-tauri/src/secrets/local.rs

#### MEDIUM

- **`local.rs:38` — `LocalEntry.value` is a plain `String`, not zeroed on drop.**
  When the keychain blob is deserialized (line 366), `blob.value` is a `String` allocated on the heap. It is immediately wrapped in `SecretString::from(blob.value)` (which does zeroize on drop), but the original heap allocation — the one from `serde_json::from_str` — is not zeroized. The blob object is dropped normally, leaving the plaintext in freed heap memory until overwritten by the allocator. This is a residue window, not an active leak. Fix: implement `Drop` on `LocalEntry` to zeroize `value`, or use `secrecy::Secret<String>` from the start.

---

### src-tauri/src/secrets/api.rs

#### MEDIUM

- **`api.rs:231-248` — `get_item_metadata` calls `store.get()`, fully decrypting the secret, then discards the value.**
  The endpoint is documented as "metadata only" and returns a `SecretRef` with no value field. But it calls `store.get()` which for the local backend reads from the OS keychain, and for Bitwarden issues a `bw get item` subprocess call that fully decrypts the vault entry. A dedicated `has()` + index lookup path (which `LocalStore` supports via its index) would be cheaper and cleaner. This also means a metadata poll inadvertently creates audit log entries in Bitwarden for each item accessed.

- **`api.rs:89-98` — `UnlockBody` fields are plain `String`s without `Zeroize` on drop.**
  `master_password`, `two_factor_token`, `api_key_client_id`, and `api_key_client_secret` are deserialized from the JSON body as `String`. They live as unprotected heap allocations until Rust drops `UnlockBody` (after the `SecretString::from(body.master_password)` conversion). The `SecretString` wraps a copy; the original `String` bytes are not zeroed. Mitigations are limited by `serde`'s API, but a note acknowledging the residue window is missing. The struct also lacks a `#[allow(dead_code)]` comment explaining why `Debug` is intentionally absent.

#### LOW

- **`api.rs:425,440` — `switch_backend` calls `load_config()` twice from disk without holding `ConfigState`.**
  Between the first `load_config()` (to persist the backend choice) and the second (to build the new store), another thread could write the config file via `update_config`. The second read would then build a store from a config that differs from what was just written. This is a narrow TOCTOU race with low probability but could result in the wrong Bitwarden server URL being used for the new store.

---

### src-tauri/src/secrets/keychain.rs

#### LOW

- **`keychain.rs:271-279` — File-mode enforcement on the `.json.tmp` file before `rename` is best-effort and relies on `rename` preserving mode.**
  `set_permissions(&tmp, 0o600)` runs before `rename(&tmp, path)`. On Linux, `rename` does preserve the mode of the source file. However, the initial `std::fs::write(&tmp, body)` creates the file with the process umask before `set_permissions` runs — so there is a brief window where the temp file is world-readable on a system with a permissive umask (e.g. 0o022). On macOS the umask default is also 0o022 so the file is created 0o644 before being clamped. Fix: open with `O_CREAT | O_WRONLY` and mode 0o600 directly, or use a library that handles this atomically.

---

### src-tauri/src/config/mod.rs

#### MEDIUM

- **`config/mod.rs:63` — `default_config_toml()` template has `enabled = false` but `ApiConfig::default()` has `enabled = true`.**
  On first run, `load_config()` serializes `AppConfig::default()` (not `default_config_toml()`) to disk, so the actual written file has `enabled = true`. But `default_config_toml()` is used as a serialization fallback (`unwrap_or_else(|_| default_config_toml())`), meaning if serialization ever fails, the written config would have `enabled = false` — silently disabling the API. The template comment `# token = "your-secret-token"` is also misleading since the token is auto-generated and should not be set manually in the file. The template should either be eliminated (only ever write from the struct) or kept strictly in sync with the struct defaults.

- **`config/mod.rs:141-145` — Malformed config falls back to `AppConfig::default()` without surfacing the error to the user.**
  A bad TOML in `config.toml` (e.g. after a hand-edit) silently returns defaults. The error is logged at `ERROR` level, which may not be visible in the UI. There is no Tauri event emitted, no notification, no visual indicator. The user sees the app "working" but with unexpected default settings. Consider emitting a `config-parse-error` event so the UI can show a banner.

#### LOW

- **`config/mod.rs:176` — `ensure_api_token` uses `unwrap_or_default()` on serialization failure, writing an empty string to the config file.**
  If `toml::to_string_pretty(&config)` fails (which should be nearly impossible but is theoretically possible if a non-serializable value is embedded), the config file is overwritten with an empty string — effectively deleting all user settings. The failure case should at minimum log an error and skip the write rather than writing empty content.

---

### src-tauri/src/config/schema/mod.rs

#### MEDIUM

- **`schema/mod.rs` at 751 lines — severe `mod.rs` violation; this file is a logic file not a facade.**
  The module contains: 15+ config struct definitions with `Default` impls, 10+ `serde` default helper functions, theme-loading functions (`get_builtin_theme`, `get_custom_theme`, `get_any_theme`, `list_all_theme_ids`, `parse_builtin_theme`, `user_themes_dir`), and a `BuiltinTheme` catalog constant. The advisory is 300 lines max for a `mod.rs`. Extractable responsibilities:
  - `theme.rs` — `ThemeConfig`, `BuiltinTheme`, `BUILTIN_THEMES`, `parse_builtin_theme`, `get_builtin_theme`, `list_builtin_theme_ids`, `get_custom_theme`, `list_custom_theme_ids`, `get_any_theme`, `list_all_theme_ids`, `user_themes_dir`.
  - `integrations.rs` — `IntegrationsConfig`, `SecretsConfig`, `BitwardenSettings`, `VoiceTranscribeConfig`, `TextHooksConfig`, their `Default` impls and private default helpers.
  - `memory.rs` — `MemoryConfig`, its `Default` impl and private default helpers.
  - `agent.rs` — `AgentConfig`, `GeneralAgentConfig`, `ProviderConfig`, `UpdaterConfig`.
  - The `mod.rs` facade should remain with only `AppConfig`, `UiConfig`, `MeshConfig`, `ApiConfig`, `SocketConfig`, `FontConfig`, `TerminalConfig`, `NotificationConfig`, `KeybindingsConfig` — all are small structs that don't warrant their own file.

- **`schema/mod.rs:607` — `TextHooksConfig.health_check_ttl_secs` uses `default_voice_health_ttl_secs` as its default function.**
  The function is named after voice, but it sets the health TTL for the text-hooks service. If the voice default ever changes independently, the text-hooks default moves with it silently. Each service should have its own named default function.

---

### src-tauri/src/secrets/mod.rs

#### MEDIUM

- **`secrets/mod.rs` at 563 lines — trait definition, types, shared validators, and 180 lines of inline tests in one file.**
  The file has four distinct concerns: (1) the `SecretString` wrapper, (2) the `SecretStore` trait, (3) `SecretError`/`StoreStatus`/request types, (4) `build_active_store`/`new_state` wiring, (5) the `validate_name` utility. The test block (lines 383-563) adds another 180 lines. Split suggestions:
  - `secret_string.rs` — `SecretString` with its `Drop`/`Debug`/`Display` impls.
  - `types.rs` — `SecretRef`, `SecretValue`, `SecretError`, `StoreStatus`, `UnlockCredentials`, `SaveRequest`, `ListFilter`, `OrganizationRef`, `CollectionRef`.
  - Keep the `SecretStore` trait, `SecretStoreState`, `validate_name`, `build_active_store` in `mod.rs` (which would then be under 150 lines).
  - Move `mod tests` to `tests.rs`.

---

### src-tauri/src/secrets/local.rs

#### LOW

- **`local.rs` at 715 lines — two distinct concerns plus a large test suite in one file.**
  The file contains: (1) `LocalStore` and its `SecretStore` impl, (2) `FileIndexStorage`/`MemIndexStorage` implementations of `IndexStorage`, (3) `IndexEntry`/`LocalIndex`/`LocalEntry` data structs, (4) 250 lines of `#[cfg(test)]`. Split: `index.rs` for the `IndexStorage` trait and both implementations; keep `LocalStore` + `LocalEntry` in `local.rs`; move tests to `local/tests.rs`.

---

### src-tauri/src/secrets/mock.rs

#### LOW

- **`mock.rs:38` — `MockEntry.value` stores secret as plain `String` without `Zeroize`.**
  This is production code (the `MockFactory` is registered in the default factory registry and can be selected by setting `secrets.backend = "mock"` in config). If a user accidentally sets this backend on a production install, secrets would be stored in heap memory without zeroing on eviction. Consider either removing `MockFactory` from `factory.rs` registry (only register in `#[cfg(test)]`) or adding a `zeroize` call in a `Drop` impl on `MockEntry`.

---

### Cross-cutting / Consistency

#### MEDIUM

- **`config/mod.rs` at 476 lines — Tauri commands, theme resolution logic, config I/O, and accessors all in one file.**
  Extractable: Tauri command handlers (`get_config`, `update_config`, `get_theme`, `list_themes`, `get_config_path`, `save_custom_theme`, `delete_custom_theme`, `get_themes_dir`) → `commands.rs`. Theme resolution (`resolve_theme`, `apply_theme_with_overrides`) → `theme.rs` (alongside or merged with schema/theme.rs). Config I/O (`load_config`, `save_config`, `ensure_api_token`, `default_config_toml`) → `io.rs`. Public accessors (`default_agent_provider`, `memory_config`, `provider_config`, `usage_tracking_enabled`) → `accessors.rs`.

- **No schema version field in `AppConfig`.** `serde(default)` handles forward compatibility (new fields get their defaults when an old config is read), but there is no protection against downgrade (new config keys are silently ignored by an older binary with `#[serde(deny_unknown_fields)]` not set). This is acceptable given the explicit no-compat policy in `CLAUDE.md`, but it means there is no mechanism to detect that a config was written by a newer binary and warn the user.

#### LOW

- **`ApiConfig.host` defaults to `"0.0.0.0"`, binding the REST API on all network interfaces.**
  With token auth enforced on every secrets endpoint this is mitigated, but the API is exposed to any device on the same LAN with no additional controls (no rate limiting, no IP allowlist). The default should arguably be `"127.0.0.1"` for a desktop tool, with users opting into external binding via config. This is especially relevant since the default `enabled = true` from the Rust struct means a fresh install immediately starts accepting connections on all interfaces.

- **`secrets/bitwarden/store.rs:211-218` — Awkward `as_deref().map(|s| s)` pattern.**
  `self.session.read().await.as_ref().map(|s| s.expose_secret().to_string()).as_deref().map(|s| s)` is a roundabout way to pass an `Option<&str>` session. The extra `.map(|s| s)` is a no-op identity transformation on `Option<&str>`. This should use `.as_deref()` directly after the `String` conversion or restructure to avoid the intermediate owned `String`. Minor clarity issue only.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 0 |
| HIGH | 3 |
| MEDIUM | 9 |
| LOW | 8 |
| **Total** | **20** |

### Notes on CRITICAL absence

No CRITICAL findings: `SecretString` correctly implements `Zeroize` on drop and suppresses `Debug`/`Display`; secrets are never passed as `bw` argv (except the 2FA code edge case rated HIGH); all REST secrets endpoints enforce `check_auth`; the `SecretValue` type carrying decrypted material is not serializable (`Debug` only, no `Serialize` derive); the index files never store secret values (tested and asserted in `local.rs:624-662`).
