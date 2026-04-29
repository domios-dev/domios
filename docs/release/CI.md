# CodeTerm — Release CI

Three independent GitHub Actions workflows produce platform bundles and
maintain the channel updater manifest. They live under
`.github/workflows/`:

| File                          | Runner(s)                | Targets                                                |
| ----------------------------- | ------------------------ | ------------------------------------------------------ |
| `release-windows.yml`         | `windows-latest`         | `x86_64-pc-windows-msvc`                               |
| `release-macos.yml`           | `macos-13`, `macos-14`   | `x86_64-apple-darwin`, `aarch64-apple-darwin`          |
| `release-linux.yml`           | `ubuntu-22.04`           | `x86_64-unknown-linux-gnu` (AppImage + .deb)           |

Each workflow is **fully self-contained** — checkout, toolchains, build,
sign (where applicable), bundle upload, manifest refresh — and uploads
its bundles to the **same draft GitHub release** identified by the tag.

## Triggers

Each workflow listens for both:

- **`push` of a tag matching `v*`** — fires all three in parallel for
  the same release tag.
- **`workflow_dispatch` with `tag` input** — re-run a single platform
  for an existing tag without retriggering the others.

## Enable / disable per platform

Each workflow has a top-level guard:

```yaml
if: ${{ vars.RELEASE_<PLATFORM>_ENABLED != 'false' }}
```

To suppress a platform without touching code, set the matching repo
**Variable** (Settings → Secrets and variables → Actions → Variables):

| Variable                  | Effect when set to `false`             |
| ------------------------- | -------------------------------------- |
| `RELEASE_WINDOWS_ENABLED` | Skips `release-windows`                |
| `RELEASE_MACOS_ENABLED`   | Skips `release-macos` (both targets)   |
| `RELEASE_LINUX_ENABLED`   | Skips `release-linux`                  |

Default is enabled — the variable does not need to exist for a workflow
to run. Only the literal string `false` disables it.

## Manual run

```bash
# Windows-only re-run for an existing tag
gh workflow run release-windows.yml -f tag=v1.1.0

# Same for macOS / Linux
gh workflow run release-macos.yml   -f tag=v1.1.0
gh workflow run release-linux.yml   -f tag=v1.1.0
```

Bundles are uploaded with `--clobber`, so manually re-running a workflow
replaces that platform's existing assets on the release.

## Updater manifest

Each workflow:

1. After the build, copies its `latest.json` fragment to
   `updater-<target>.json` and uploads it to the release.
2. Downloads every `updater-*.json` fragment currently on the release,
   merges them via `scripts/merge-latest-json.js`, and uploads the
   merged result as the channel manifest:
   - `latest.json` for stable tags (`vX.Y.Z`)
   - `latest-rc.json` for `*-rc*` tags
   - `latest-nightly.json` for `*-nightly*` tags

The merge step is eventually consistent: the last workflow to finish
publishes the fully-merged manifest. Intermediate runs may publish a
manifest containing only the platforms that have completed so far —
this is intentional and harmless because Tauri's updater simply finds
no entry for a platform that hasn't been built yet.

## Required secrets

| Secret                       | Used by             | Notes |
| ---------------------------- | ------------------- | ----- |
| `TAURI_SIGNING_PRIVATE_KEY`  | All three           | Updater signing key. |
| `TAURI_KEY_PASSWORD`         | All three (optional)| Only required if the signing key is encrypted; the workflow tolerates an empty value. |
| `APPLE_CERTIFICATE`          | macOS               | Base64 `.p12` for Developer ID. |
| `APPLE_CERTIFICATE_PASSWORD` | macOS               | Password for the `.p12`. |
| `APPLE_ID`                   | macOS               | Apple ID email used for notarytool. |
| `APPLE_PASSWORD`             | macOS               | App-specific password for notarization. |
| `APPLE_TEAM_ID`              | macOS               | 10-char Apple Developer team ID. |
| `APPLE_SIGNING_IDENTITY`     | macOS               | `Developer ID Application: …` Common Name. |

## Out of scope (future work)

- Shared composite action for the duplicated build/sign/upload steps —
  three near-identical workflows are deliberate for now to keep each
  platform debuggable in isolation.
- Aggregating the manifest in a fourth workflow triggered by
  `workflow_run` completion of the three platform workflows. The
  inline merge is sufficient for the current release cadence.
- Cross-compiling Linux ARM (`aarch64-unknown-linux-gnu`).

<!-- revision: 1 -->
