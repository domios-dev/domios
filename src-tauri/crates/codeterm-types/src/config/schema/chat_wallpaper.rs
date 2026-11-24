use serde::{de::Error, Deserialize, Deserializer, Serialize};
use ts_rs::TS;

use super::{ChatBackground, ChatPattern};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatWallpaper {
    pub background: ChatBackground,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub palette: Option<WallpaperPalette>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fill: Option<WallpaperFill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fill_strength: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperPalette {
    Aurora,
    Sunset,
    Ocean,
    Orchid,
    Ember,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct WallpaperFill {
    pub mode: WallpaperFillMode,
    pub preset: WallpaperFillPreset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperFillMode {
    Solid,
    Gradient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperFillPreset {
    Midnight,
    Dusk,
    DeepSea,
    Plum,
    Forest,
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ChatWallpaper>, D::Error> {
    Option::<ChatWallpaper>::deserialize(deserializer)?
        .map(|mut wallpaper| {
            wallpaper.validate().map_err(D::Error::custom)?;
            Ok(wallpaper)
        })
        .transpose()
}

impl ChatWallpaper {
    pub fn validate(&mut self) -> Result<(), &'static str> {
        if self
            .fill_strength
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("Wallpaper background strength must be between 0 and 1");
        }
        if let Some(asset) = &self.asset {
            match (self.background.pattern, wallpaper_asset_format(asset)) {
                (ChatPattern::Image, Some("png")) | (ChatPattern::Svg, Some("svg")) => {}
                _ => return Err("Wallpaper asset does not match the selected pattern"),
            }
        }
        if self.asset_label.as_ref().is_some_and(|label| {
            self.asset.is_none()
                || label.is_empty()
                || label.len() > 255
                || label
                    .chars()
                    .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
        }) {
            return Err("Wallpaper file label must be a plain filename");
        }
        self.background.validate(self.asset.is_some())
    }
}

pub fn wallpaper_asset_format(name: &str) -> Option<&str> {
    let (hash, extension) = name.split_once('.')?;
    (hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        && matches!(extension, "png" | "svg"))
    .then_some(extension)
}

#[cfg(test)]
mod tests;
