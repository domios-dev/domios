# Themes

CodeTerm supports built-in and custom themes. A theme is a TOML file with 20 color fields.

## CLI

```bash
codeterm theme list                     # builtin + custom
codeterm theme apply dracula
codeterm theme dir                      # show custom themes directory
codeterm theme save my-theme /tmp/my-theme.toml [--apply]
codeterm theme delete my-theme
```

## Creating a custom theme

Write a `.toml` file with all 20 color fields, save it, then apply:

```bash
cat > /tmp/my-theme.toml << 'EOF'
name = "My Custom Theme"
background = "#1a1b26"
foreground = "#c0caf5"
cursor = "#c0caf5"
selection = "#33467c"
black = "#15161e"
red = "#f7768e"
green = "#9ece6a"
yellow = "#e0af68"
blue = "#7aa2f7"
magenta = "#bb9af7"
cyan = "#7dcfff"
white = "#a9b1d6"
bright_black = "#414868"
bright_red = "#f7768e"
bright_green = "#9ece6a"
bright_yellow = "#e0af68"
bright_blue = "#7aa2f7"
bright_magenta = "#bb9af7"
bright_cyan = "#7dcfff"
bright_white = "#c0caf5"
EOF

codeterm theme save my-theme /tmp/my-theme.toml --apply
```

UI colors (borders, sidebar, surfaces) are automatically derived from the background. Light themes get darkened borders; dark themes get lightened ones.
