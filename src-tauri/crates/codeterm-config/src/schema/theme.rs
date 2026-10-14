use super::ThemeConfig;

/// Built-in theme definitions loaded at compile time.
pub struct BuiltinTheme {
    pub id: &'static str,
    pub toml_str: &'static str,
}

pub const BUILTIN_THEMES: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: "github-dark",
        toml_str: include_str!("../../../../themes/github-dark.toml"),
    },
    BuiltinTheme {
        id: "dracula",
        toml_str: include_str!("../../../../themes/dracula.toml"),
    },
    BuiltinTheme {
        id: "catppuccin-mocha",
        toml_str: include_str!("../../../../themes/catppuccin-mocha.toml"),
    },
    BuiltinTheme {
        id: "one-dark",
        toml_str: include_str!("../../../../themes/one-dark.toml"),
    },
    BuiltinTheme {
        id: "nord",
        toml_str: include_str!("../../../../themes/nord.toml"),
    },
    BuiltinTheme {
        id: "github-light",
        toml_str: include_str!("../../../../themes/github-light.toml"),
    },
    BuiltinTheme {
        id: "solarized-light",
        toml_str: include_str!("../../../../themes/solarized-light.toml"),
    },
    BuiltinTheme {
        id: "pip-boy",
        toml_str: include_str!("../../../../themes/pip-boy.toml"),
    },
];

/// Parse a built-in theme TOML string into a ThemeConfig.
pub fn parse_builtin_theme(toml_str: &str) -> Result<ThemeConfig, String> {
    toml::from_str(toml_str).map_err(|e| format!("Failed to parse theme: {}", e))
}

/// Get a built-in theme by name. Returns None if not found.
pub fn get_builtin_theme(name: &str) -> Option<ThemeConfig> {
    BUILTIN_THEMES
        .iter()
        .find(|t| t.id == name)
        .and_then(|t| parse_builtin_theme(t.toml_str).ok())
}

/// List all available built-in theme IDs.
pub fn list_builtin_theme_ids() -> Vec<String> {
    BUILTIN_THEMES.iter().map(|t| t.id.to_string()).collect()
}

/// Directory for user custom themes.
pub fn user_themes_dir() -> Option<std::path::PathBuf> {
    Some(codeterm_paths::data_dir().join("themes"))
}

/// Load a custom theme from the user themes directory.
pub fn get_custom_theme(name: &str) -> Option<ThemeConfig> {
    let dir = user_themes_dir()?;
    let path = dir.join(format!("{}.toml", name));
    if path.exists() {
        let content = std::fs::read_to_string(&path).ok()?;
        parse_builtin_theme(&content).ok()
    } else {
        None
    }
}

/// List all custom theme IDs from the user themes directory.
pub fn list_custom_theme_ids() -> Vec<String> {
    let Some(dir) = user_themes_dir() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".toml").map(|s| s.to_string())
        })
        .collect()
}

/// Get a theme by name — checks builtins first, then user custom themes.
pub fn get_any_theme(name: &str) -> Option<ThemeConfig> {
    get_builtin_theme(name).or_else(|| get_custom_theme(name))
}

/// List all theme IDs (builtins + custom).
pub fn list_all_theme_ids() -> Vec<String> {
    let mut ids = list_builtin_theme_ids();
    ids.extend(list_custom_theme_ids());
    ids
}
