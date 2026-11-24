use codeterm_service_core::config::schema::{
    AppearanceLook, ChatPattern, ThemeConfig, WallpaperFillMode, WallpaperFillPreset,
    WallpaperPalette,
};
use serde::Serialize;

const TEXT: f64 = 4.5;
const NON_TEXT: f64 = 3.0;

#[derive(Debug, Serialize)]
pub(super) struct ContrastCheck {
    pub name: &'static str,
    pub pair: String,
    pub ratio: f64,
    pub floor: f64,
    pub fix: Option<ContrastFix>,
}

#[derive(Debug, Serialize)]
pub(super) struct ContrastFix {
    field: &'static str,
    value: serde_json::Value,
}

type Color = [f64; 3];
const BLACK: Color = [0.0; 3];
const WHITE: Color = [255.0; 3];

fn color(value: &str) -> Result<Color, String> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(format!("Expected #rrggbb, got {value}"));
    };
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("Expected #rrggbb, got {value}"));
    }
    let number = u32::from_str_radix(hex, 16).map_err(|error| error.to_string())?;
    Ok([
        ((number >> 16) & 255) as f64,
        ((number >> 8) & 255) as f64,
        (number & 255) as f64,
    ])
}

fn luminance(color: Color) -> f64 {
    color
        .into_iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(channel, weight)| {
            let value = channel / 255.0;
            weight
                * if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
        })
        .sum()
}

fn ratio(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn mix(a: Color, b: Color, amount: f64) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * amount)
}

// Matches deriveColor in src/lib/colors.ts by preserving hue and saturation.
fn derive(color: Color, amount: f64) -> Color {
    let low = color.into_iter().fold(f64::INFINITY, f64::min);
    let high = color.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let light = (low + high) / 2.0;
    let sign = if color.iter().sum::<f64>() / 3.0 > 128.0 {
        -1.0
    } else {
        1.0
    };
    let next = (light + amount * sign * 1.02).clamp(0.0, 255.0);
    let spread = light.min(255.0 - light);
    if spread == 0.0 {
        return [next.round(); 3];
    }
    let factor = next.min(255.0 - next) / spread;
    color.map(|channel| (next + (channel - light) * factor).round())
}

fn check(
    name: &'static str,
    surface: &str,
    ink: Color,
    backgrounds: &[Color],
    floor: f64,
) -> ContrastCheck {
    let ink_luminance = luminance(ink);
    let mut backgrounds_luminance = backgrounds.iter().copied().map(luminance);
    let first = backgrounds_luminance.next().unwrap_or_default();
    let (low, high) = backgrounds_luminance.fold((first, first), |(low, high), value| {
        (low.min(value), high.max(value))
    });
    let ratio = if low <= ink_luminance && ink_luminance <= high {
        1.0
    } else {
        backgrounds
            .iter()
            .map(|surface| ratio(ink, *surface))
            .fold(f64::INFINITY, f64::min)
    };
    ContrastCheck {
        name,
        pair: format!("{name} on {surface}"),
        ratio,
        floor,
        fix: None,
    }
}

pub(super) fn validate_palette(theme: &ThemeConfig) -> Result<(), String> {
    let palette = [
        &theme.background,
        &theme.foreground,
        &theme.cursor,
        &theme.selection,
        &theme.black,
        &theme.red,
        &theme.green,
        &theme.yellow,
        &theme.blue,
        &theme.magenta,
        &theme.cyan,
        &theme.white,
        &theme.bright_black,
        &theme.bright_red,
        &theme.bright_green,
        &theme.bright_yellow,
        &theme.bright_blue,
        &theme.bright_magenta,
        &theme.bright_cyan,
        &theme.bright_white,
    ];
    for value in palette {
        color(value)?;
    }
    Ok(())
}

pub(super) fn theme_is_light(theme: &ThemeConfig) -> bool {
    color(&theme.background).is_ok_and(is_light)
}

pub(super) fn measure(look: &AppearanceLook) -> Result<Vec<ContrastCheck>, String> {
    let theme = &look.theme;
    validate_palette(theme)?;
    let base = color(&theme.background)?;
    let ink = color(&theme.foreground)?;
    let accent = color(&theme.blue)?;
    let muted = mix(color(&theme.bright_black)?, ink, 0.45).map(f64::round);
    let surfaces = [
        base,
        derive(base, 4.0),
        derive(base, 8.0),
        derive(base, 10.0),
    ];
    let bubble = mix(surfaces[3], accent, 0.08).map(f64::round);
    let text_surfaces = [surfaces.as_slice(), &[bubble]].concat();
    let text = [
        ("Text", ink, "theme.foreground", ink),
        (
            "Muted",
            muted,
            "theme.bright_black",
            color(&theme.bright_black)?,
        ),
        ("Accent", accent, "theme.blue", accent),
        (
            "Success",
            color(&theme.green)?,
            "theme.green",
            color(&theme.green)?,
        ),
        (
            "Warning",
            color(&theme.yellow)?,
            "theme.yellow",
            color(&theme.yellow)?,
        ),
        ("Error", color(&theme.red)?, "theme.red", color(&theme.red)?),
    ];
    let mut checks = vec![ContrastCheck {
        name: "Button label",
        pair: "Button label on accent".into(),
        ratio: ratio(BLACK, accent).max(ratio(WHITE, accent)),
        floor: TEXT,
        fix: None,
    }];
    for (index, background) in text_surfaces.iter().enumerate() {
        let name = [
            "base",
            "titlebar",
            "sidebar",
            "raised surface",
            "user bubble",
        ][index];
        for (label, effective, field, source) in text {
            if index == 4 && label != "Text" && label != "Muted" {
                continue;
            }
            let mut check = check(label, name, effective, &[*background], TEXT);
            if check.ratio < check.floor {
                check.fix = suggested_color(field, source, &text_surfaces, TEXT, |color| {
                    if label == "Muted" {
                        mix(color, ink, 0.45).map(f64::round)
                    } else {
                        color
                    }
                });
            }
            checks.push(check);
        }
        let mut focus = check("Focus ring", name, accent, &[*background], NON_TEXT);
        if focus.ratio < focus.floor {
            focus.fix = suggested_color("theme.blue", accent, &text_surfaces, NON_TEXT, |color| {
                color
            });
        }
        checks.push(focus);
        checks.push(check("Control edge", name, ink, &[*background], NON_TEXT));
    }
    let paper = wallpaper_surfaces(look, base, ink)?;
    for (label, foreground, field, source) in text {
        let mut entry = check(label, "wallpaper", foreground, &paper, TEXT);
        if entry.ratio < entry.floor {
            let no_marks = wallpaper_fill_surfaces(look, base)?;
            entry.fix = if check(label, "wallpaper fill", foreground, &no_marks, TEXT).ratio >= TEXT
            {
                let source = look
                    .wallpaper
                    .as_ref()
                    .map(|paper| &paper.background)
                    .or(theme.chat_background.as_ref());
                let fix_field = if look.wallpaper.is_some() {
                    "chat_wallpaper.background.opacity"
                } else {
                    "theme.chat_background.opacity"
                };
                let start =
                    (source.and_then(|paper| paper.opacity).unwrap_or(0.3) * 100.0).ceil() as u8;
                let mut edited = look.clone();
                let opacity = (0..start)
                    .rev()
                    .find(|percent| {
                        let background = if let Some(paper) = edited.wallpaper.as_mut() {
                            &mut paper.background
                        } else {
                            edited
                                .theme
                                .chat_background
                                .get_or_insert_with(Default::default)
                        };
                        background.opacity = Some(f64::from(*percent) / 100.0);
                        wallpaper_surfaces(&edited, base, ink).is_ok_and(|surfaces| {
                            check(label, "wallpaper", foreground, &surfaces, TEXT).ratio >= TEXT
                        })
                    })
                    .unwrap_or(0);
                Some(ContrastFix {
                    field: fix_field,
                    value: serde_json::json!(f64::from(opacity) / 100.0),
                })
            } else if look
                .wallpaper
                .as_ref()
                .is_some_and(|paper| paper.fill.is_some() || paper.background.gradient.is_some())
            {
                let mut edited = look.clone();
                let strength = (0..20)
                    .rev()
                    .map(|step| f64::from(step) / 20.0)
                    .find(|candidate| {
                        if let Some(wallpaper) = edited.wallpaper.as_mut() {
                            wallpaper.fill_strength = Some(*candidate);
                        }
                        wallpaper_surfaces(&edited, base, ink).is_ok_and(|surfaces| {
                            check(label, "wallpaper", foreground, &surfaces, TEXT).ratio >= TEXT
                        })
                    });
                strength.map(|value| ContrastFix {
                    field: "chat_wallpaper.fill_strength",
                    value: serde_json::json!(value),
                })
            } else {
                suggested_color(field, source, &paper, TEXT, |color| {
                    if label == "Muted" {
                        mix(color, ink, 0.45).map(f64::round)
                    } else {
                        color
                    }
                })
            };
        }
        checks.push(entry);
    }
    Ok(checks)
}

fn suggested_color(
    field: &'static str,
    source: Color,
    backgrounds: &[Color],
    floor: f64,
    resolve: impl Fn(Color) -> Color,
) -> Option<ContrastFix> {
    for step in 1..=100 {
        for extreme in [BLACK, WHITE] {
            let candidate = mix(source, extreme, f64::from(step) / 100.0).map(f64::round);
            if check("", "", resolve(candidate), backgrounds, floor).ratio >= floor {
                return Some(ContrastFix {
                    field,
                    value: serde_json::json!(format!(
                        "#{:02x}{:02x}{:02x}",
                        candidate[0] as u8, candidate[1] as u8, candidate[2] as u8
                    )),
                });
            }
        }
    }
    None
}

fn is_light(base: Color) -> bool {
    base[0] * 0.299 + base[1] * 0.587 + base[2] * 0.114 > 150.0
}

fn wallpaper_fill_surfaces(look: &AppearanceLook, base: Color) -> Result<Vec<Color>, String> {
    let background = look
        .wallpaper
        .as_ref()
        .map(|paper| &paper.background)
        .or(look.theme.chat_background.as_ref());
    let mut surfaces = vec![base];
    let strength = look
        .wallpaper
        .as_ref()
        .and_then(|paper| paper.fill_strength)
        .unwrap_or(1.0);
    if !(0.0..=1.0).contains(&strength) {
        return Err("Wallpaper background strength must be between 0 and 1".into());
    }
    if let Some(fill) = look
        .wallpaper
        .as_ref()
        .and_then(|paper| paper.fill.as_ref())
    {
        use WallpaperFillPreset::*;
        let light = is_light(base);
        let (first, second) = match fill.preset {
            Midnight => (base, color(if light { "#e9eef8" } else { "#141b33" })?),
            Dusk => (color(if light { "#f4effb" } else { "#1b1430" })?, base),
            DeepSea => (color(if light { "#eaf5f8" } else { "#0a2230" })?, base),
            Plum => (color(if light { "#fbeff4" } else { "#26132a" })?, base),
            Forest => (color(if light { "#ecf6ef" } else { "#0f2218" })?, base),
        };
        surfaces = match fill.mode {
            WallpaperFillMode::Solid => vec![mix(base, first, strength)],
            WallpaperFillMode::Gradient => {
                channel_bounds(&[mix(base, first, strength), mix(base, second, strength)])
            }
        };
    } else if let Some(gradient) = background.and_then(|value| value.gradient.as_ref()) {
        let opacity = if look.wallpaper.is_some() {
            strength
        } else {
            0.08
        };
        for stop in &gradient.stops {
            surfaces.push(mix(base, color(stop)?, opacity));
        }
        surfaces = channel_bounds(&surfaces);
    }
    Ok(surfaces)
}

fn wallpaper_surfaces(
    look: &AppearanceLook,
    base: Color,
    ink: Color,
) -> Result<Vec<Color>, String> {
    let wallpaper = look.wallpaper.as_ref();
    let light = base[0] * 0.299 + base[1] * 0.587 + base[2] * 0.114 > 150.0;
    let background = wallpaper
        .map(|paper| &paper.background)
        .or(look.theme.chat_background.as_ref());
    let pattern = background.map(|value| value.pattern).unwrap_or_default();
    let palette = wallpaper.and_then(|paper| paper.palette);
    let painted = palette.is_some()
        || background.is_some_and(|paper| paper.color.is_some() || paper.colors.is_some());
    let defaults = match pattern {
        ChatPattern::Dots => {
            if light {
                0.035
            } else {
                0.028
            }
        }
        ChatPattern::Image => 0.30,
        ChatPattern::Svg if wallpaper.is_some() => {
            if light {
                0.06
            } else {
                0.12
            }
        }
        ChatPattern::Grid | ChatPattern::Icon if wallpaper.is_some() => {
            if light {
                0.06
            } else {
                0.05
            }
        }
        ChatPattern::Grid | ChatPattern::Icon | ChatPattern::Svg => {
            if light {
                0.035
            } else {
                0.028
            }
        }
        ChatPattern::Doodles if wallpaper.is_none() => {
            if light {
                0.06
            } else {
                0.085
            }
        }
        _ if painted => {
            if light {
                0.16
            } else {
                0.22
            }
        }
        _ => {
            if light {
                0.06
            } else {
                0.085
            }
        }
    };
    let alpha = background
        .and_then(|value| value.opacity)
        .unwrap_or(defaults)
        .min(if pattern == ChatPattern::Image {
            0.30
        } else {
            0.24
        });
    let mut surfaces = wallpaper_fill_surfaces(look, base)?;
    if pattern == ChatPattern::None || alpha == 0.0 {
        return Ok(surfaces);
    }
    let tints =
        if pattern == ChatPattern::Image || (pattern == ChatPattern::Svg && wallpaper.is_none()) {
            vec![BLACK, WHITE]
        } else if let Some(palette) = palette {
            wallpaper_palette(palette, light)
                .iter()
                .map(|value| color(value))
                .collect::<Result<_, _>>()?
        } else if let Some(stops) = background.and_then(|paper| paper.colors.as_ref()) {
            stops
                .iter()
                .map(|value| color(value))
                .collect::<Result<_, _>>()?
        } else {
            vec![background
                .and_then(|value| value.color.as_deref())
                .map(color)
                .transpose()?
                .unwrap_or(ink)]
        };
    for surface in surfaces.clone() {
        for tint in channel_bounds(&tints) {
            surfaces.push(mix(surface, tint, alpha));
        }
    }
    Ok(channel_bounds(&surfaces))
}

fn channel_bounds(colors: &[Color]) -> Vec<Color> {
    let low = std::array::from_fn(|i| colors.iter().map(|color| color[i]).fold(255.0, f64::min));
    let high = std::array::from_fn(|i| colors.iter().map(|color| color[i]).fold(0.0, f64::max));
    vec![low, high]
}

fn wallpaper_palette(palette: WallpaperPalette, light: bool) -> &'static [&'static str] {
    use WallpaperPalette::*;
    match (palette, light) {
        (Aurora, false) => &["#7ee8fa", "#80ff9a", "#b49bff"],
        (Aurora, true) => &["#0a7ea4", "#2b8a3e", "#6d48c8"],
        (Sunset, false) => &["#ffb199", "#ff6a88", "#ffd27a"],
        (Sunset, true) => &["#c2410c", "#be185d", "#a16207"],
        (Ocean, false) => &["#4facfe", "#00e0ff"],
        (Ocean, true) => &["#1d4ed8", "#0e7490"],
        (Orchid, false) => &["#c3a6ff", "#ff9ad5"],
        (Orchid, true) => &["#6d28d9", "#be185d"],
        (Ember, false) => &["#ffc46b", "#ff5f6d"],
        (Ember, true) => &["#b45309", "#b91c1c"],
        (Mono, false) => &["#9aa4ae"],
        (Mono, true) => &["#5c6670"],
    }
}
