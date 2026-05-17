---
title: macOS x86_64 release build (full embeddings, cross-compile)
area: deploy
tags: [release, macos, x86_64, ort, fastembed, notarize, cross-compile]
---

# macOS x86_64 release build with full embeddings

How to cross-compile a macOS Intel DMG that ships the full fastembed / vector-search feature, on an arm64 Mac.

## The problem

`ort-sys` (ONNX Runtime Rust bindings) does not provide `x86_64-apple-darwin` prebuilt binaries for any 2.x version — pyke dfbin only has arm64 mac + linux + windows. `fastembed` hard-pins `ort = "=2.0.0-rc.11"` so bumping ort doesn't help. `cargo build --target x86_64-apple-darwin` fails immediately at `ort-sys` build script.

arm64 build is unaffected — it uses the default `ort/download-binaries` feature and statically links ONNX Runtime.

## The fix (committed as `66b6e5a`)

1. **Target-conditional fastembed feature** in `src-tauri/Cargo.toml`:
   ```toml
   [target.'cfg(all(target_os = "macos", target_arch = "x86_64"))'.dependencies]
   fastembed = { version = "5", optional = true, features = ["ort-load-dynamic"] }
   ```
   Cargo unifies this with the base dep → on x86_64-mac only, adds `ort-load-dynamic` → ort stops trying to link ONNX at compile time, loads a dylib at runtime instead. arm64 dep resolution is identical.

2. **Runtime dylib locator** in `src-tauri/src/bin/codeterm_embed.rs`:
   ```rust
   #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
   fn ensure_ort_dylib() {
       // Find libonnxruntime.dylib next to the binary or in Contents/Resources/
       // and set ORT_DYLIB_PATH before TextEmbedding::try_new() runs.
   }
   ```
   Looks for the dylib in:
   - sibling to the binary
   - `../Resources/embed-libs/libonnxruntime.dylib` (Tauri 2 macOS layout)
   - `../Resources/_up_/embed-libs/libonnxruntime.dylib` (legacy `_up_` layout)
   - `../Frameworks/libonnxruntime.dylib`

3. **Bundle the dylib in the .app** via a merge config — keeps arm64 DMG lean.

## Steps to rebuild

Use `scripts/build.sh`. It does every step below in order. You need:
- arm64 Mac with Xcode
- `rustup target add x86_64-apple-darwin`
- `APPLE_SIGNING_IDENTITY` + notarization creds in `~/.claude/.secrets/apple-notary.env`
- ONNX Runtime 1.23.2 Intel dylib (downloaded once, cached under `/tmp/`)

One-liner:
```bash
bash scripts/release-macos-x86_64.sh (from repo root)
```

## Gotchas

- **ONNX Runtime 1.24+ dropped Intel mac support** — use 1.23.2 exactly. Newer versions have no `osx-x86_64` tarball.
- **Dylib must be codesigned with hardened runtime** (same identity as the app) to pass notarization when bundled into the .app. `codesign --force --options runtime --timestamp --sign "Developer ID Application: ..."`.
- **Notarize the DMG itself**, not just the .app inside. Tauri auto-notarizes the .app during `pnpm tauri build`, but the DMG wrapper needs a separate `xcrun notarytool submit <dmg>` call. Last release v0.10.0 skipped this and shipped a broken DMG.
- **Verify with `spctl --assess --type open --context context:primary-signature --verbose <dmg>`** — must say `accepted, source=Notarized Developer ID`. Anything else = broken release.
- **Don't bundle the dylib into the arm64 DMG** — it bloats the arm64 build by ~39 MB for no reason. The `cfg(all(target_os = "macos", target_arch = "x86_64"))` gate ensures arm64 never runs `ensure_ort_dylib` and the merge config only applies to x86_64 builds.
- **Test ABI compat once** — ort 2.0.0-rc.11 was built against ONNX Runtime 1.24 API, and 1.23.2 dylib is forward-compatible with the slice fastembed uses. If upstream ort bumps to a new major ONNX API, this compat may break; re-verify with a roundtrip embedding test.
- **CARGO_BUILD_TARGET is load-bearing for the cross-build.** On an arm64 host, `pnpm tauri build --target x86_64-apple-darwin` runs `beforeBuildCommand` which calls `cargo build --manifest-path cli/Cargo.toml` (no --target) → builds for the HOST arch and ships arm64 CLI inside the x86_64 .app. The release script exports `CARGO_BUILD_TARGET=x86_64-apple-darwin` before invoking `pnpm tauri build` so every embedded cargo invocation cross-builds. `scripts/bundle-cli.js` and `scripts/bundle-embed.js` also read `CARGO_BUILD_TARGET` to pick the right `cli/target/<TARGET>/release/...` source dir and emit `bundled-embed/codeterm-embed-<TARGET>`. Removing the env export OR breaking that bundle-script logic re-introduces issue #113 silently.
- **TAURI_SIGNING_PRIVATE_KEY must be set** or Tauri skips the `.app.tar.gz` + `.sig` updater bundle entirely — `latest.json`'s `darwin-x86_64.signature` would then stay stale and auto-update propagation to Intel users breaks. The release script reads `~/.tauri/codeterm.key` (empty-password local-dev minisign key) into the env before `pnpm tauri build`.
- **Suppressing `beforeBuildCommand` is wrong now.** Earlier versions of the merge config set `"beforeBuildCommand": ""` to keep tauri from clobbering pre-staged binaries — but that also skipped rebuilding `dist/` AND prevented bundle-cli.js from picking up `CARGO_BUILD_TARGET`. With the current bundle scripts the right behavior is to let `beforeBuildCommand` run normally; the env var forces the cross-build.
- **Always assert the bundled CLI arch before notarize.** The release script runs `file Contents/Resources/cli/codeterm | grep -q x86_64` (and the same for `Contents/MacOS/codeterm`) right after `pnpm tauri build` and aborts on mismatch — that's the failure mode v1.3.0 hit (issue #113), and the assertion makes it impossible to ship the same regression silently again.

## Files touched (one-time)

- `src-tauri/Cargo.toml` — target-conditional fastembed dep
- `src-tauri/src/bin/codeterm_embed.rs` — `ensure_ort_dylib()` function
- `.gitignore` — ignore `src-tauri/bundled-embed-libs/` and `.fastembed_cache/`

## Verification roundtrip

After producing the x86_64 `codeterm-embed` binary and the signed dylib:
```bash
ORT_DYLIB_PATH=/tmp/onnxruntime-osx-x86_64-1.23.2/lib/libonnxruntime.1.23.2.dylib \
  arch -x86_64 src-tauri/target/x86_64-apple-darwin/release/codeterm-embed
# then on stdin:
{"id":"test","text":"hello world"}
```
Should return a 384-dim embedding. Then run the bundled binary inside the .app — it must produce the exact same vector.

## Homebrew tap

`rollacode/homebrew-codeterm/Casks/codeterm.rb` uses `on_arm` / `on_intel` blocks; update both `sha256`s per release. After a new release:
```bash
gh api repos/rollacode/homebrew-codeterm/contents/Casks/codeterm.rb --jq .content | base64 -d
```

## Context

This workaround landed in commit `66b6e5a` (2026-04-15) for release v1.0.0. Prior to that we shipped an FTS5-only x86_64 DMG (20 MB, no semantic search) as a fallback for v1.0.0-rc.

## v1.3.0 regression (issue #113) — what shipped wrong, what we changed

**Symptom**: After `codeterm update --apply` on Intel Macs, the bundled CLI died with `Bad CPU type in executable`. `file Contents/Resources/cli/codeterm` reported `arm64` inside an otherwise-x86_64 .app bundle. GUI binary was correct.

**Root cause**: `scripts/release-macos-x86_64.sh` pre-staged the right x86_64 CLI at step 4 (`cp cli/target/x86_64-apple-darwin/release/codeterm src-tauri/bundled-cli/codeterm`), then invoked `pnpm tauri build`. `beforeBuildCommand` (which the merge config tried to suppress with `""` but Tauri still honored as a no-op-style empty value here) ran `cargo build --release --manifest-path cli/Cargo.toml` WITHOUT `--target`. On the arm64 build host this produced an arm64 binary at `cli/target/release/codeterm`, and `scripts/bundle-cli.js` then overwrote the pre-staged x86_64 binary with that arm64 one. Tauri bundled the corrupted `bundled-cli/codeterm` into the x86_64 .app.

**Fix** (committed alongside this skill update):

1. `scripts/bundle-cli.js` + `scripts/bundle-embed.js` now respect `CARGO_BUILD_TARGET`: when set they read from `<crate>/target/<TARGET>/release/...` and (for the embed sidecar) emit `bundled-embed/codeterm-embed-<TARGET>` so the suffix matches what Tauri looks up for the cross-build. Backwards-compatible: if `CARGO_BUILD_TARGET` is unset they behave exactly as before.
2. `scripts/release-macos-x86_64.sh` exports `CARGO_BUILD_TARGET=x86_64-apple-darwin` + `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""` immediately before `pnpm tauri build`, then unsets them after.
3. The merge config no longer overrides `beforeBuildCommand` — letting it run is what lets `pnpm build` regenerate `dist/` AND what causes bundle-cli.js / bundle-embed.js to re-read the env var and stage the correct-arch binaries.
4. The script pre-creates empty stubs at `bundled-embed/codeterm-embed-x86_64-apple-darwin`, `bundled-embed/codeterm-embed-<HOST_TRIPLE>`, and `bundled-cli/codeterm` before the standalone cargo builds in step 3b — those cargo invocations run `tauri_build::build()` which validates `externalBin` + `resources` globs against the filesystem.
5. **Hard arch assertion** runs immediately after `pnpm tauri build` (step 6b):
   ```bash
   file Contents/Resources/cli/codeterm | grep -q x86_64 || { echo "FATAL: ..."; exit 1; }
   file Contents/MacOS/codeterm        | grep -q x86_64 || { echo "FATAL: ..."; exit 1; }
   ```
   This means a future regression that bundles the wrong-arch CLI fails the release script BEFORE the costly notarize step — and certainly before any upload happens.

**Verify a future build hasn't regressed**:
```bash
tar -xzf CodeTerm_<ver>_x64.app.tar.gz
file CodeTerm.app/Contents/MacOS/codeterm           # must be x86_64
file CodeTerm.app/Contents/Resources/cli/codeterm   # must be x86_64
```
Or pull the live release: `gh release download <tag> --repo rollacode/codeterm-releases --pattern 'CodeTerm_*_x64.app.tar.gz'` and run the same `file` checks.
