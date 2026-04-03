# Release Process

## Machines

| Machine | OS | Artifacts |
|---------|-----|-----------|
| **MacBook Air** | macOS arm64 | DMG aarch64, DMG x64, CLI macOS (both arch) |
| **BLACKSTAR** | Windows x64 | EXE/MSI x64, CLI Windows |

Both machines have `.env` in project root with `UPDATER_TOKEN` (gitignored).

## Steps

### 1. Bump version

Edit version in 3 files (must match):
- `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
- `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
- `package.json` → `"version": "X.Y.Z"`

Commit and push to master:
```bash
git add -A
git commit -m "release: vX.Y.Z"
git push origin master
```

### 2. Create draft release on GitHub

```bash
gh release create vX.Y.Z --draft --title "CodeTerm vX.Y.Z" --notes "release notes here"
```

Draft release is now visible. Both machines can upload artifacts to it.

### 3. Build and upload — each machine independently

#### MacBook Air (macOS)

```bash
git pull origin master
./build.sh build                                          # DMG aarch64
pnpm tauri build --target x86_64-apple-darwin             # DMG x64

# CLI
cargo build --release --manifest-path cli/Cargo.toml
cp cli/target/release/codeterm codeterm-cli-macos-aarch64
cargo build --release --manifest-path cli/Cargo.toml --target x86_64-apple-darwin
cp cli/target/x86_64-apple-darwin/release/codeterm codeterm-cli-macos-x86_64

# Upload
gh release upload vX.Y.Z \
  src-tauri/target/release/bundle/dmg/CodeTerm_*_aarch64.dmg \
  src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_*_x64.dmg \
  codeterm-cli-macos-aarch64 \
  codeterm-cli-macos-x86_64
```

#### BLACKSTAR (Windows)

```bash
git pull origin master
./build.sh build

# CLI
cargo build --release --manifest-path cli/Cargo.toml
copy cli\target\release\codeterm.exe codeterm-cli-windows-x86_64.exe

# Upload
gh release upload vX.Y.Z ^
  src-tauri\target\release\bundle\nsis\CodeTerm_*_x64-setup.exe ^
  codeterm-cli-windows-x86_64.exe
```

### 4. Verify all artifacts are uploaded

```bash
gh release view vX.Y.Z --json assets -q '.assets[].name'
```

Expected:
- `CodeTerm_X.Y.Z_aarch64.dmg`
- `CodeTerm_X.Y.Z_x64.dmg`
- `CodeTerm_X.Y.Z_x64-setup.exe`
- `codeterm-cli-macos-aarch64`
- `codeterm-cli-macos-x86_64`
- `codeterm-cli-windows-x86_64.exe`

### 5. Publish

```bash
gh release edit vX.Y.Z --draft=false
```

### 6. Update Homebrew tap (macOS only)

```bash
# Get hashes
sha256sum CodeTerm_*_aarch64.dmg CodeTerm_*_x64.dmg

# Clone tap, update Casks/codeterm.rb, push
gh repo clone rollacode/homebrew-codeterm
cd homebrew-codeterm
# Edit Casks/codeterm.rb — update version + sha256 hashes
git commit -am "codeterm X.Y.Z" && git push
```

### 7. Sync develop

```bash
git checkout develop
git merge master
git push origin develop
```

## Notes

- **No CI for builds.** Everything is built locally on the two machines.
- **No tags needed.** `gh release create vX.Y.Z` creates the tag automatically when published.
- **Draft first.** Always create draft before building, so both machines have a target to upload to.
- **Order doesn't matter.** MacBook and BLACKSTAR can build and upload in any order.
- **.env must exist** in project root on each build machine for `UPDATER_TOKEN` to be baked in.
