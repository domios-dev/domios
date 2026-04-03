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
# Clone
git clone <repo-url> codeterm
cd codeterm

# Install frontend dependencies
pnpm install

# Development mode (hot-reload)
npx tauri dev

# Production build
npx tauri build
```

## Dev/Prod Isolation

Dev builds (`npx tauri dev`) run with `tauri.dev.conf.json` overrides:
- **Identifier:** `com.codeterm.dev` (vs `com.codeterm.app` in prod)
- **API port:** 7686 (vs 7685 in prod)
- **Data directory:** `~/.codeterm-dev/` (vs `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (vs `codeterm`)
- **Single-instance:** Disabled (dev and prod can run side by side)

## macOS Code Signing

**Release builds MUST be signed.** Unsigned DMGs trigger Gatekeeper ("unidentified developer").

```bash
# Signing identity (required for release builds)
APPLE_SIGNING_IDENTITY="Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)"

# Build signed DMG
APPLE_SIGNING_IDENTITY="Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)" pnpm tauri build
APPLE_SIGNING_IDENTITY="Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)" pnpm tauri build --target x86_64-apple-darwin
```

Add to `.env` in project root (gitignored) so `build.sh` picks it up automatically:
```
UPDATER_TOKEN=<github PAT>
APPLE_SIGNING_IDENTITY=Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)
```

For notarization (optional, removes "downloaded from internet" warning):
```bash
APPLE_ID=<apple-id-email>
APPLE_PASSWORD=<app-specific-password>
APPLE_TEAM_ID=JMMWQ9LWF7
```

## Build Output

After `npx tauri build`:

| File | Location |
|------|----------|
| **Portable exe** | `src-tauri/target/release/codeterm.exe` |
| **NSIS installer** | `src-tauri/target/release/bundle/nsis/CodeTerm_0.1.0_x64-setup.exe` |
| **MSI installer** | `src-tauri/target/release/bundle/msi/CodeTerm_0.1.0_x64_en-US.msi` |
| **CLI binary** | `cli/target/release/codeterm.exe` (build separately) |

## Building CLI separately

```bash
cargo build --release --manifest-path cli/Cargo.toml
```

CLI binary: `cli/target/release/codeterm.exe`

## Running Tests

```bash
# Rust unit + integration tests (46 tests)
cargo test --manifest-path src-tauri/Cargo.toml

# Frontend type check
npx tsc --noEmit

# E2E tests (requires running app)
npx tauri dev &
sleep 90
bash tests/final_e2e.sh
```

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
