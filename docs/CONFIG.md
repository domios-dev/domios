# CodeTerm Configuration

Config file: `%APPDATA%/codeterm/config.toml` (Windows) or `~/.config/codeterm/config.toml` (Linux/macOS).

Auto-created on first run with defaults.

## Full Reference

```toml
# CodeTerm Configuration

[theme]
name = "github-dark"
# Available: github-dark, dracula, catppuccin-mocha, one-dark, nord
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
# Default keybindings (not yet customizable):
# Ctrl+Shift+T  = New tab
# Ctrl+Shift+W  = Close tab
# Ctrl+Shift+D  = Split right
# Ctrl+Shift+E  = Split down
# Ctrl+Shift+Q  = Close pane
# Ctrl+1-9      = Switch tab
# Ctrl+,        = Settings

[socket]
# Auto-generated, override if needed:
# path = "\\\\.\\pipe\\codeterm-custom"

[agent]
poll_interval_ms = 500
idle_timeout_s = 30

[api]
enabled = true
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

## Session Persistence

Sessions auto-save when you close the window. Saved to:
- `%APPDATA%/codeterm/sessions/last.json`

Contains workspace layout, tab arrangement, and split tree structure.
