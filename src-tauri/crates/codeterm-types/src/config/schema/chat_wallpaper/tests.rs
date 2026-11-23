use super::*;
use crate::config::schema::{AppConfig, ThemeConfig};

#[test]
fn chat_wallpaper_round_trips_without_modifying_the_theme() {
    let config: AppConfig = toml::from_str(
        r##"
        [theme.chat_background]
        pattern = "grid"

        [chat_wallpaper]
        palette = "aurora"

        [chat_wallpaper.background]
        pattern = "space"
        opacity = 0.22
        scale = 360

        [chat_wallpaper.fill]
        mode = "gradient"
        preset = "deep_sea"
        "##,
    )
    .unwrap();
    let reloaded: AppConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(config.chat_wallpaper, reloaded.chat_wallpaper);
    assert_eq!(
        reloaded.theme.chat_background.unwrap().pattern,
        ChatPattern::Grid
    );
    assert!(AppConfig::default().chat_wallpaper.is_none());
}

#[test]
fn chat_wallpaper_assets_are_hash_only_and_reserved_for_user_settings() {
    let image_id = format!("{}.png", "f".repeat(64));
    let config = |asset: &str, pattern: &str| {
        serde_json::from_value::<AppConfig>(serde_json::json!({
            "chat_wallpaper": {
                "background": {"pattern": pattern},
                "asset": asset
            }
        }))
    };
    assert!(config(&image_id, "image").is_ok());
    for asset in [
        "../escape.png",
        "https://example.invalid/file.png",
        ".png",
        "file:///x.png",
    ] {
        assert!(config(asset, "image").is_err(), "{asset}");
    }
    assert!(config(&image_id, "svg").is_err());
    assert!(config(&image_id, "space").is_err());
    assert!(serde_json::from_value::<ThemeConfig>(serde_json::json!({
        "chat_background": {"pattern": "image"}
    }))
    .is_err());
}

#[test]
fn chat_wallpaper_curated_art_passes_the_same_static_svg_policy() {
    for (name, art) in [
        (
            "domios",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-domios.svg"),
        ),
        (
            "agents",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-agents.svg"),
        ),
        (
            "space",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-space.svg"),
        ),
        (
            "math",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-math.svg"),
        ),
        (
            "botanical",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-botanical.svg"),
        ),
        (
            "retro",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-retro.svg"),
        ),
        (
            "geometry",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-geometry.svg"),
        ),
        (
            "sketch",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-sketch.svg"),
        ),
        (
            "circuits",
            include_str!("../../../../../../../src/assets/chat-wallpapers/domios-circuits.svg"),
        ),
    ] {
        let clean = crate::svg::sanitize_svg(art.as_bytes(), crate::svg::SvgUse::ChatTile);
        assert!(
            clean.is_some_and(|svg| svg.contains("stroke=\"currentColor\"")),
            "{name}"
        );
    }
}
