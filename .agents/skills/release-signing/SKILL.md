---
name: release-signing
description: "Configure, rotate, and protect CodeTerm updater signing keys and CI secrets."
---

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
`plugins.updater.pubkey`. Routine releases use the configured key. Never
replace a live public key directly; rotation requires the dual-key transition
described below.

## GitHub Secrets

The release workflows use these repository secrets:

- `TAURI_SIGNING_PRIVATE_KEY` — full contents of `~/.tauri/codeterm.key`
- `TAURI_KEY_PASSWORD` — optional password for an encrypted key

The workflows pass these to Tauri as `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; Tauri signs updater artifacts and emits
their `.sig` files.

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

## Release procedure
Use the current end-to-end sequence in
`.agents/skills/github-release/SKILL.md`. Do not tag or publish from this
signing setup guide.

## Channel naming convention

The workflow detects channel from the tag suffix:

| Tag pattern              | Channel  | Manifest               | `prerelease` flag |
|--------------------------|----------|------------------------|-------------------|
| `vX.Y.Z`                 | stable   | `latest.json`          | false             |
| `vX.Y.Z-rc.N`            | rc       | `latest-rc.json`       | true              |
| `vX.Y.Z-nightly.YYYYMMDD`| nightly  | `latest-nightly.json`  | true              |

Stay within these patterns so client-side channel selection in
`updater::channel` resolves the correct manifest URL.

## Build features in CI

Release builds are produced **without** `dev-updater-allowlocal`. That
feature whitelists `file://` and `http://127.0.0.1:*` manifest URLs and
exists strictly for local end-to-end testing of the apply pipeline. CI
never enables it, so production binaries refuse anything other than the
HTTPS GitHub-hosted manifest.

## Per-user install on Windows

The Windows NSIS bundle uses `bundle.windows.nsis.installMode = "currentUser"`
in `src-tauri/tauri.conf.json`. It installs under `%LOCALAPPDATA%` without
admin elevation; system-wide install mode is not configured.
