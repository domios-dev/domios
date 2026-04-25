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

## How to ship a release

End-to-end flow once secrets are configured:

1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
   and `package.json` to the new value (e.g. `1.0.2`). Commit on `master`.
2. Tag the commit and push the tag:

   ```bash
   git tag v1.0.2
   git push origin v1.0.2
   ```

3. The push triggers `.github/workflows/release.yml`, which:
   - builds `windows-latest`, `macos-13`, and `macos-14` in parallel,
   - signs every asset with `TAURI_PRIVATE_KEY` via `tauri-action`,
   - notarizes macOS bundles with the `APPLE_*` secrets,
   - publishes a **draft** GitHub release containing the binaries plus a
     per-platform `latest.json` fragment.
4. The follow-up `publish-manifest` job downloads those fragments,
   merges them via `scripts/merge-latest-json.js`, and uploads a single
   channel manifest to the same release. Filename depends on the tag:
   - `v1.2.3`             → `latest.json`         (stable channel)
   - `v1.2.3-rc.1`        → `latest-rc.json`      (rc channel)
   - `v1.2.3-nightly.YYYYMMDD` → `latest-nightly.json` (nightly channel)
5. Review the draft on GitHub, then click "Publish release". Users on the
   matching channel see "Update available" within 5 minutes (configurable
   poll cadence in client).

Manual trigger via `workflow_dispatch` is available for re-running a
specific tag from the Actions tab; pass the tag name as the input.

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

Both NSIS and WiX bundles are configured for per-user installation in
`tauri.conf.json` (`bundle.windows.nsis.installMode = "perUser"`,
`bundle.windows.wix.installScope = "perUser"`). The installer writes to
`%LOCALAPPDATA%` and does not require admin elevation. A future
`--all-users` switch (system-wide install) is documented in
`docs/plans/auto-update.md` but not implemented in v1.
