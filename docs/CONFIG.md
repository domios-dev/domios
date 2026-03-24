# CodeTerm Configuration

Config file: `%APPDATA%/codeterm/config.toml` (Windows) or `~/.config/codeterm/config.toml` (Linux/macOS).

Auto-created on first run with defaults.

## Full Reference

```toml
# CodeTerm Configuration

[theme]
name = "github-dark"
# Available: github-dark, github-light, dracula, catppuccin-mocha, one-dark, nord, solarized-light, pip-boy
# Override individual colors:
# background = "#0d1117"
# foreground = "#c9d1d9"
# cursor = "#58a6ff"

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"  # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[notifications]
desktop = true
sound = false
toast_duration = 5  # seconds

[keybindings]
new_tab = "Ctrl+Shift+T"
close_tab = "Ctrl+Shift+W"
next_tab = "Ctrl+Tab"
prev_tab = "Ctrl+Shift+Tab"
split_right = "Ctrl+Shift+D"
split_down = "Ctrl+Shift+E"
close_pane = "Ctrl+Shift+Q"
focus_pane_left = "Ctrl+Alt+Left"
focus_pane_right = "Ctrl+Alt+Right"
focus_pane_up = "Ctrl+Alt+Up"
focus_pane_down = "Ctrl+Alt+Down"
new_workspace = "Ctrl+Shift+N"
copy = "Ctrl+Shift+C"
paste = "Ctrl+Shift+V"
search = "Ctrl+Shift+F"
zoom_in = "Ctrl+="
zoom_out = "Ctrl+-"
zoom_reset = "Ctrl+0"
open_browser = "Ctrl+Shift+B"
open_settings = "Ctrl+,"

[socket]
# Auto-generated, override if needed:
# path = "\\\\.\\pipe\\codeterm-custom"

[agent]
# Agent config controls AI agent integration (e.g. Claude Code).
# poll_interval_ms: how often the agent polls for new commands.
# idle_timeout_s: disconnect agent after this many seconds of inactivity.
poll_interval_ms = 500
idle_timeout_s = 30

[api]
enabled = true
host = "0.0.0.0"
port = 7685
# token = "your-secret-token"  # Uncomment to require auth
cors = true
```

## Themes

### GitHub Dark (default)
Dark theme matching GitHub's dark mode.

### Dracula
Popular purple-tinted dark theme.

### Catppuccin Mocha
Warm pastel dark theme.

### One Dark
Atom-inspired dark theme.

### Nord
Arctic blue-tinted theme.

### GitHub Light
Light theme matching GitHub's light mode.

### Solarized Light
Ethan Schoonover's warm light color scheme.

### Pip-Boy
Retro green-on-black Fallout terminal theme. Selecting this theme automatically activates CRT effects: scanlines, phosphor glow, and screen vignette.

## Session Persistence

Sessions auto-save every 30 seconds and when you close the window. Saved to:
- **Windows:** `%APPDATA%/codeterm/sessions/last.json`
- **Linux:** `~/.config/codeterm/sessions/last.json`
- **macOS:** `~/Library/Application Support/codeterm/sessions/last.json`

Contains workspace layout, tab arrangement, and split tree structure.

## CWD Tracking

CodeTerm tracks the current working directory of each pane via invisible OSC 7 shell integration. No visible commands are injected into the terminal.

**How it works per shell:**
- **zsh** (macOS default): ZDOTDIR trick -- a temporary `.zshrc` in the integration directory sources the user's real config, then adds a `precmd` hook that emits OSC 7.
- **bash** (Linux default): `PROMPT_COMMAND` env var is set at PTY spawn time, plus `BASH_ENV` points to an integration script that sources `~/.bashrc` first.
- **PowerShell** (Windows default): Prompt injection after spawn (same as before).

Shell integration scripts are stored in the platform data directory:
- **macOS:** `~/Library/Application Support/codeterm/shell-integration/`
- **Linux:** `~/.local/share/codeterm/shell-integration/`
- **Windows:** `%APPDATA%/codeterm/shell-integration/`

Scripts are created at startup by `pty::shell_integration::ensure_scripts()`. Users should not need to edit these files.

<!-- revision: 4 -->
