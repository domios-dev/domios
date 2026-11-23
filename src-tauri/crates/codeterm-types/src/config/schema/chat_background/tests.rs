use super::*;
use crate::config::schema::{AppConfig, ThemeConfig};

#[test]
fn chat_background_is_optional_and_defaults_to_dots() {
    assert!(toml::from_str::<ThemeConfig>("name = 'minimal'")
        .unwrap()
        .chat_background
        .is_none());
    let theme: ThemeConfig = toml::from_str("[chat_background]").unwrap();
    assert_eq!(theme.chat_background.unwrap().pattern, ChatPattern::Dots);
    let theme: ThemeConfig = toml::from_str("[chat_background]\npattern = 'doodles'").unwrap();
    assert_eq!(theme.chat_background.unwrap().pattern, ChatPattern::Doodles);
}

#[test]
fn chat_background_round_trips_as_app_config() {
    let config: AppConfig = toml::from_str(
        r##"
        [theme.chat_background]
        pattern = "icon"
        icon = "cat"
        scale = 48
        opacity = 0.75
        color = "#4455aa"
        rotation = -15

        [theme.chat_background.gradient]
        kind = "linear"
        angle = 35
        stops = ["#4455aa", "#3c876f", "#a34d63"]
        "##,
    )
    .unwrap();
    let wallpaper = config.theme.chat_background.as_ref().unwrap();
    assert_eq!(wallpaper.pattern, ChatPattern::Icon);
    assert_eq!(wallpaper.icon, Some(ChatIcon::Cat));
    assert_eq!(wallpaper.scale, Some(48));
    assert_eq!(wallpaper.rotation, Some(-15.0));
    let saved = toml::to_string(&config).unwrap();
    let loaded: AppConfig = toml::from_str(&saved).unwrap();
    assert_eq!(loaded.theme.chat_background.as_ref(), Some(wallpaper));
}

#[test]
fn chat_background_rejects_out_of_range_and_invalid_fields() {
    for (source, field) in [
        ("pattern = 'stars'", "pattern"),
        ("pattern = 'icon'\nicon = 'unknown'", "icon"),
        ("scale = 7", "scale"),
        ("scale = 513", "scale"),
        ("opacity = -0.01", "opacity"),
        ("opacity = 1.01", "opacity"),
        ("opacity = nan", "opacity"),
        ("color = '#fff'", "color"),
        ("rotation = 361", "rotation"),
        ("rotation = inf", "rotation"),
        ("pattern = 'svg'", "svg"),
        (
            "[chat_background.gradient]\nkind = 'linear'\nstops = ['#123456']",
            "stops",
        ),
        (
            "[chat_background.gradient]\nkind = 'radial'\nstops = ['#123456', 'red']",
            "stops",
        ),
    ] {
        let source = if source.starts_with('[') {
            source.to_owned()
        } else {
            format!("[chat_background]\n{source}")
        };
        let error = toml::from_str::<ThemeConfig>(&source).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
    for (scale, opacity) in [(8, 0), (512, 1)] {
        let theme = format!("[chat_background]\nscale = {scale}\nopacity = {opacity}");
        assert!(toml::from_str::<ThemeConfig>(&theme).is_ok(), "{theme}");
    }
}

#[test]
fn chat_background_sanitizes_static_svg_and_refuses_active_svg() {
    let valid = r#"<svg width="24" height="24" viewBox="0 0 24 24">
        <g fill="currentColor"><circle cx="12" cy="12" r="4" /></g>
        </svg>"#;
    let parse = |svg: &str| {
        toml::from_str::<ThemeConfig>(&format!(
            "[chat_background]\npattern='svg'\nsvg='''{svg}'''"
        ))
    };
    let clean = parse(valid).unwrap().chat_background.unwrap().svg.unwrap();
    assert_eq!(
        clean,
        r#"<svg viewBox="0 0 24 24"><g fill="currentColor"><circle cx="12" cy="12" r="4"/></g></svg>"#
    );
    for body in [
        "<script/>",
        "<foreignObject/>",
        "<animate attributeName=\"opacity\"/>",
        "<style/>",
        "<image href=\"https://example.test/image.png\"/>",
        "<path onclick=\"void(0)\"/>",
        "<path fill=\"url(#local)\"/>",
        "<path stroke=\"&#117;rl(#local)\"/>",
        "<path xmlns:x=\"http://www.w3.org/1999/xlink\"/>",
        "<?xml-stylesheet href=\"data:text/css,\"?>",
        "<path>",
    ] {
        let svg = format!(r#"<svg viewBox="0 0 24 24">{body}</svg>"#);
        assert!(parse(&svg).is_err(), "{svg}");
    }
    assert!(parse("<!DOCTYPE svg><svg viewBox=\"0 0 24 24\"/>").is_err());
    assert!(parse("<svg/>").is_err());
    assert!(parse(&" ".repeat(crate::svg::MAX_CHAT_TILE_BYTES + 1)).is_err());
    assert!(serde_json::from_value::<ThemeConfig>(serde_json::json!({
        "chat_background": { "pattern": "svg", "svg": "<svg><script/></svg>" }
    }))
    .is_err());
}

#[test]
fn chat_background_accepts_a_large_stroke_tile_with_static_transforms() {
    let paths = r#"<path d="M0 10 C4 3 12 2 20 10 S40 26 50 10"/>"#.repeat(300);
    let source = format!(
        r#"[chat_background]
pattern = "svg"
scale = 512
svg = '''<svg viewBox="0 0 512 512" fill="none" stroke="currentColor">
<g transform="translate(25,12) rotate(-10)">{paths}</g>
</svg>'''
"#
    );
    let theme: ThemeConfig = toml::from_str(&source).unwrap();
    let tile = theme.chat_background.unwrap().svg.unwrap();
    assert!(tile.len() > crate::svg::MAX_SVG_BYTES);
    assert_eq!(tile.matches("<path").count(), 300);
    assert!(tile.contains(r#"stroke="currentColor""#));
    assert!(tile.contains(r#"transform="translate(25,12) rotate(-10)""#));
}

#[test]
fn chat_background_bundled_doodles_meet_the_static_tile_policy() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../src/lib/chatDoodles.ts"
    ));
    let (_, tile) = source.split_once('`').unwrap();
    let (tile, _) = tile.rsplit_once('`').unwrap();
    let sanitized = sanitize_svg(tile.as_bytes(), SvgUse::ChatTile).unwrap();
    assert!(sanitized.contains(r#"viewBox="0 0 512 512""#));
    assert!(sanitized.contains(r#"stroke="currentColor""#));
}
