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
