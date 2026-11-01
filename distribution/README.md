# Domios package definitions

These are unpublished templates for the proposed `domios-dev/scoop-domios`
bucket (`bucket/domios.json`) and `domios-dev/homebrew-domios` tap
(`Casks/domios.rb`). Repository creation and publication require owner approval.
The `@VERSION@` and `@SHA256_...@` values deliberately prevent installation.
Do not replace a checksum with `skip`, invent a digest, or publish these templates.

## Release inputs

Use one accepted release run, its pinned source SHA, retained desktop artifacts,
and `manifest-local-verification.json` from the existing release evidence owner.
Asset names follow `scripts/release-dry-run-manifest.mjs` and the platform workflows:

| Package architecture | Desktop asset for version X.Y.Z | Bundled CLI | Bundled server |
| --- | --- | --- | --- |
| Scoop 64bit | `Domios_X.Y.Z_x64-setup.exe` | `cli/domios.exe`, `cli/codeterm.exe` | `domios-server/domios-server.exe` |
| Scoop arm64 | `Domios_X.Y.Z_arm64-setup.exe` | `cli/domios.exe`, `cli/codeterm.exe` | `domios-server/domios-server.exe` |
| Homebrew Intel | `Domios_X.Y.Z_x64.dmg` | `Domios.app/Contents/Resources/cli/{domios,codeterm}` | `Domios.app/Contents/MacOS/domios-server` |
| Homebrew Apple Silicon | `Domios_X.Y.Z_aarch64.dmg` | `Domios.app/Contents/Resources/cli/{domios,codeterm}` | `Domios.app/Contents/MacOS/domios-server` |

Every URL is version pinned under
`https://github.com/domios-dev/domios/releases/download/vX.Y.Z/`.
The NSIS installer and DMG carry the existing matching desktop/server pair;
neither package installs a headless archive or a separate updater. The GUI is
`domios-app[.exe]`; only the bundled CLI becomes `domios` and `codeterm` on PATH.
Homebrew installs `Domios.app` and links its resource CLIs. Scoop runs the native
per-user NSIS installer into its version directory and shims its resource CLIs.
Scoop refuses a global install and an uninstall/upgrade while that install is running.

## Fill the placeholders

After the release evidence owner accepts the retained artifacts, run this offline
step in the authorized canonical QA environment, replacing the four arguments:

```sh
node distribution/prepare.mjs SOURCE_SHA /accepted/manifest-local-verification.json /accepted/retained-inputs /review/empty-output
```

The retained input root contains one subdirectory per platform from the table,
using the release platform keys `windows-x86_64`, `windows-aarch64`,
`darwin-x86_64`, and `darwin-aarch64`. Keep the original captured filenames.
The helper reads the existing schema-1 aggregate receipt, requires the supplied
source SHA and one matching platform receipt for each architecture, hashes all four
nonempty desktop assets, and compares size and SHA256 with the receipts before
writing anything. It fills every version/hash token in both definitions and writes
`package-inputs.json` with the selected filenames, public URLs, sizes and digests.
It refuses foreign/missing/duplicate receipts, changed bytes and a nonempty output.

This verifies consistency with supplied evidence; it does not authenticate that
evidence, validate signatures, publish packages or prove the native installation.
The release owner must supply accepted provenance and signing/notarization results.
At publication, anonymously download each exact public URL into a separate
directory and compare all four SHA256 values to `package-inputs.json` and the
accepted retained receipts. Do not hash the macOS updater `.app.tar.gz` in place
of the DMG. Retain the public readback and original source/signature receipts.
No automatic `autoupdate`, `livecheck` or checksum bypass is included.

## Canonical and disposable native verification

All checks are deferred to the Orchestrator; Workers run none on the shared machine.

Before external package repository writes, use owner-authorized disposable native
Windows and macOS environments with their installed package manager. Record tool
versions, commands, raw exits, selected architecture, paths and full CLI identity.
Use the prepared files in a local checkout with the proposed bucket/tap layout.

Windows, separately on x64 and ARM64:

```powershell
scoop --version
scoop help install
.\bin\test.ps1
scoop install ./bucket/domios.json
Get-Command domios,codeterm -All
where.exe domios
where.exe codeterm
domios identify
codeterm identify
```

Use the [official bucket template's test entry point](https://github.com/ScoopInstaller/BucketTemplate/blob/master/bin/test.ps1)
in the local bucket checkout, including its Pester tests and required BuildHelpers
and Pester modules. Record the reviewed template commit and module versions;
missing lint tooling is an explicit unmet gate.
Install only in a disposable user profile. Prove that NSIS honors `/D` for both
architectures, the installed pair and `server-component.json` exist under the
Scoop version directory, and the two CLI files have identical SHA256. Inspect
Scoop's `.shim` targets as well as command discovery; a shim path alone is not proof.
Launch the desktop there and record actual `domios-app.exe` and
`domios-server.exe` process paths, matching version/build and CLI identity.
Check upgrade/uninstall while stopped, including native uninstaller and firewall
cleanup. Do not enable Scoop publication if NSIS path or lifecycle behavior fails.

macOS, separately on Intel and Apple Silicon after registering the prepared local
tap checkout as `domios-dev/domios` in that disposable environment:

```sh
brew --version
brew style domios-dev/domios/domios
brew audit --cask --strict domios-dev/domios/domios
brew install --cask domios-dev/domios/domios
command -v domios
command -v codeterm
domios identify
codeterm identify
```

Verify symlink destinations are the installed app's `Contents/Resources/cli/`
executables, both CLI digests agree, the bundle identifier is `com.domios.app`,
and strict signing/Gatekeeper assessment passes. Launch the app and prove actual
client/server process paths inside that same bundle and matching version/build.
Check uninstall and reinstall with no stale CLI links. The app may also refresh
its user CLI copies at startup; inspect all PATH matches and their digests.
Preserve those receipts before asking to create/publish the two external repositories.

The owner retained the updater/plugin mirror for 1.16.1; package downloads use
the public source release. See [the classified consumer inventory](url-consumers.md).
Release hashes, public readback, native lint/audit, both architectures' install
smokes and final CI remain UNVERIFIED until their owning gates execute.
