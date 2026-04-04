# CodeTerm — Build Guide

## Prerequisites

### Windows
- **Rust** 1.93+ (`rustup install stable`)
- **Node.js** 22+ with **pnpm** (`npm install -g pnpm`)
- **MSVC Build Tools** — Visual Studio 2022 Build Tools with:
  - `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`
  - `Microsoft.VisualStudio.Component.Windows11SDK.22621`
- **WebView2** — comes with Windows 11, install separately on Windows 10

### macOS
- Xcode Command Line Tools
- Rust, Node.js, pnpm

### Linux
- `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libgtk-3-dev`
- Rust, Node.js, pnpm

## Quick Start

```bash
git clone <repo-url> codeterm && cd codeterm
pnpm install
./build.sh dev     # development (hot-reload)
./build.sh build   # production build
```

## Dev/Prod Isolation

Dev builds (`npx tauri dev`) run with `tauri.dev.conf.json` overrides:
- **Identifier:** `com.codeterm.dev` (vs `com.codeterm.app` in prod)
- **API port:** 7686 (vs 7685 in prod)
- **Data directory:** `~/.codeterm-dev/` (vs `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (vs `codeterm`)
- **Single-instance:** Disabled (dev and prod can run side by side)

## Build Output

After `./build.sh build`:

| File | Location |
|------|----------|
| **NSIS installer** | `src-tauri/target/release/bundle/nsis/CodeTerm_X.Y.Z_x64-setup.exe` |
| **MSI installer** | `src-tauri/target/release/bundle/msi/CodeTerm_X.Y.Z_x64_en-US.msi` |
| **Portable exe** | `src-tauri/target/release/codeterm.exe` |
| **CLI binary** | bundled automatically by `build.sh` into `~/.codeterm/bin/` |

## Running Tests

```bash
./build.sh test                                            # Rust unit tests (~307)
npx tsx src/components/ChatView/parser.test.ts             # TS parser tests
./build.sh e2e                                             # E2E (needs running dev instance)
```

## Release Process

No CI — all builds are local, coordinated via CodeTerm mesh between two machines.

### Workflow

The release is driven by one machine (lead), the other builds in parallel (support). Both machines are connected via CodeTerm mesh. The lead handles version bump, tagging, draft creation, and publish. The support machine pulls, builds, and uploads its platform installer.

Typical setup: **MacBook Air** (lead, macOS ARM) + **BLACKSTAR** (support, Windows x64). Either machine can lead — just coordinate via mesh `send-text`.

### Steps

1. **Lead: bump version** in 3 files: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
2. **Lead: commit + push + tag:**
   ```bash
   git add -A && git commit -m "release: vX.Y.Z — summary"
   git tag vX.Y.Z && git push origin master --tags
   ```
3. **Lead: create draft release** (before building — so both machines can upload in parallel):
   ```bash
   gh release create vX.Y.Z --draft --title "vX.Y.Z" --notes "changelog here"
   ```
4. **Lead: tell support to build** (via mesh):
   ```bash
   codeterm send-text "git pull && ./build.sh build && gh release upload vX.Y.Z <installer-path> --clobber" --pane general --mesh BLACKSTAR
   ```
5. **Both machines build in parallel:**
   - **Lead** runs `./build.sh build` locally → uploads its installer
   - **Support** pulls, builds, uploads its installer
   ```bash
   gh release upload vX.Y.Z path/to/installer --clobber
   ```
6. **Lead: publish** when all machines have uploaded their artifacts:
   ```bash
   gh release edit vX.Y.Z --draft=false
   ```

### Build artifacts

| Platform | Installer | Path |
|----------|-----------|------|
| Windows x64 | NSIS `.exe` | `src-tauri/target/release/bundle/nsis/CodeTerm_X.Y.Z_x64-setup.exe` |
| macOS ARM | `.dmg` | `src-tauri/target/release/bundle/dmg/CodeTerm_X.Y.Z_aarch64.dmg` |
| macOS Intel | `.dmg` | `src-tauri/target/release/bundle/dmg/CodeTerm_X.Y.Z_x64.dmg` |

### Machines

- **BLACKSTAR** — Windows builds (RTX 4090, fast compile)
- **MacBook Air** — macOS ARM builds (personal, `rollacode`)
- **MacBook Pro** — macOS Intel/ARM builds if needed (`ainyiva`)

## Icons

Icon source files: `src-tauri/icons/`

| File | Size | Usage |
|------|------|-------|
| `icon.png` | 512x512 | Window icon, installer |
| `256x256.png` | 256x256 | High-DPI displays |
| `32x32.png` | 32x32 | System tray (Windows/Linux) |
| `tray-template.png` | — | macOS tray (template image, auto light/dark) |
| `icon.ico` | 256x256 | Windows executable |

**Bundle icons** (`tauri.conf.json` → `bundle.icon`) are embedded into the built installer/exe.

**Runtime icons** are set programmatically in `src/lib.rs` setup — needed for dev mode and custom title bar:
- Window icon: decoded from `icon.png` via `png` crate → `window.set_icon()`
- Tray icon: decoded from `tray-template.png` (macOS, white on transparent, auto-adapts to light/dark) or `32x32.png` (Windows/Linux) → `TrayIconBuilder::icon()`

### Regenerating icons

1. Edit SVG source or create new design
2. Render to PNG at multiple sizes using `sharp` (Node.js) or ImageMagick
3. Replace files in `src-tauri/icons/`
4. Rebuild — icons are embedded at compile time via `include_bytes!()`

### Windows icon cache

If the icon doesn't update after rebuild, clear Windows icon cache:
```bash
rm -f "$LOCALAPPDATA/IconCache.db"
rm -rf "$LOCALAPPDATA/Microsoft/Windows/Explorer/iconcache*"
rm -rf "$LOCALAPPDATA/Microsoft/Windows/Explorer/thumbcache*"
taskkill /F /IM explorer.exe && start explorer.exe
```

## Windows MSVC Notes

If build fails with `link.exe` errors, ensure MSVC is properly configured. The project includes `.cargo/config.toml` with MSVC paths. You may need to update paths if your MSVC version differs.

Check your MSVC installation:
```powershell
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

## Project Structure

```
codeterm/
├── src-tauri/          # Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs      # App entry, state setup
│   │   ├── commands.rs  # PTY Tauri commands
│   │   ├── pty/         # PTY manager (portable-pty)
│   │   ├── multiplexer/ # Workspace/Tab/Pane state
│   │   ├── agent/       # Agent detection, git info
│   │   ├── socket/      # JSON-RPC socket server
│   │   ├── api/         # REST API (Axum)
│   │   ├── notification/# Notification store
│   │   ├── config/      # TOML config + themes
│   │   ├── prompts.rs   # Agent prompt templates
│   │   └── session/     # Session persistence
│   └── Cargo.toml
├── src/                # React frontend
│   ├── App.tsx
│   ├── components/
│   │   ├── Terminal/    # xterm.js, splits
│   │   ├── Sidebar/     # Workspace/tab list
│   │   ├── Titlebar/    # Custom titlebar
│   │   ├── Notifications/
│   │   └── Settings/
│   ├── store/           # React context state
│   └── hooks/
├── cli/                # CLI binary
├── skills/             # Claude Code skills
├── tests/              # E2E tests
└── docs/               # Documentation
```
