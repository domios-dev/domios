use codeterm_service_core::config::schema::{parse_builtin_theme, ThemeConfig};

pub(super) fn all() -> Vec<ThemeConfig> {
    [
        include_str!("../../../../themes/github-dark.toml"),
        include_str!("../../../../themes/github-light.toml"),
        include_str!("../../../../themes/appearance/forest-night.toml"),
        include_str!("../../../../themes/appearance/warm-retro.toml"),
        include_str!("../../../../themes/appearance/deep-ocean.toml"),
        include_str!("../../../../themes/appearance/paper-ink.toml"),
        include_str!("../../../../themes/appearance/nordic-fog.toml"),
        include_str!("../../../../themes/appearance/sunrise.toml"),
    ]
    .into_iter()
    .map(|source| parse_builtin_theme(source).expect("bundled appearance preset"))
    .collect()
}
