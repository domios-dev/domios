# Auto-update signing

CodeTerm uses `tauri-plugin-updater` with ed25519 signatures. Every release asset
is signed with the project's private key; the public key is embedded in the
binary via `tauri.conf.json` (`plugins.updater.pubkey`). Anyone can attempt to
host a fake `latest.json`, but without the matching private key the client
refuses to apply the update.

## One-time keypair generation

```
cargo tauri signer generate -w ~/.tauri/codeterm.key
```

The command prompts for a password. It writes:

- `~/.tauri/codeterm.key` — encrypted private key (never commit)
- `~/.tauri/codeterm.key.pub` — public key (base64, safe to share)

Copy the contents of `codeterm.key.pub` into `src-tauri/tauri.conf.json` under
`plugins.updater.pubkey`. The current value is a placeholder copied from a
sample project — generate a real key and replace it before producing the first
signed release.

## GitHub Secrets

Two repository secrets feed `tauri-action` in CI:

- `TAURI_PRIVATE_KEY` — full contents of `~/.tauri/codeterm.key`
- `TAURI_KEY_PASSWORD` — the password used at generation time

`tauri-action` reads them from the environment, signs each platform asset, and
uploads the signed `.sig` files alongside the binaries.

## Local convention

Keep keypairs under `~/.tauri/`. That directory is conventional for Tauri
tooling and is intentionally outside the repository tree. Add it to your
shell's backup/exclude list as needed; never let it get committed.

## Rotation

Generating a new keypair invalidates every prior signed release: clients on
old binaries can verify only the original public key, so they cannot
auto-update through a key change. The supported workaround is a transition
release that bundles **both** the old and the new public key — Tauri accepts
an array for `plugins.updater.pubkey`. Ship the transition release first, let
users move to it, then publish the next release signed solely with the new
key.

If a key is ever leaked, rotate it immediately and treat all releases prior
to the rotation as compromised.
