use serde::{de::Error, Deserialize, Deserializer, Serialize};
use ts_rs::TS;

use crate::svg::{sanitize_svg, SvgUse};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatPattern {
    #[default]
    None,
    Dots,
    Grid,
    Icon,
    Svg,
    Doodles,
    Domios,
    Space,
    Math,
    Botanical,
    Retro,
    Geometry,
    Sketch,
    Circuits,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatIcon {
    Cat,
    Diamond,
    Star,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct ChatBackground {
    pub pattern: ChatPattern,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scale: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub colors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<ChatIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gradient: Option<ChatGradient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatGradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatGradient {
    pub kind: ChatGradientKind,
    pub stops: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub angle: Option<f64>,
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ChatBackground>, D::Error> {
    Option::<ChatBackground>::deserialize(deserializer)
        .map_err(|error| D::Error::custom(format_args!("chat_background: {error}")))?
        .map(|mut background| {
            background.validate(false).map_err(D::Error::custom)?;
            Ok(background)
        })
        .transpose()
}

impl ChatBackground {
    pub(super) fn validate(&mut self, has_asset: bool) -> Result<(), &'static str> {
        if self.scale.is_some_and(|value| !(8..=512).contains(&value)) {
            return Err("chat_background.scale must be between 8 and 512 px");
        }
        if self
            .opacity
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("chat_background.opacity must be between 0 and 1");
        }
        if self.color.as_deref().is_some_and(|value| !hex_color(value)) {
            return Err("chat_background.color must be #rrggbb");
        }
        if self.colors.as_ref().is_some_and(|stops| {
            !(1..=3).contains(&stops.len()) || !stops.iter().all(|stop| hex_color(stop))
        }) {
            return Err("chat_background.colors must contain one to three #rrggbb colors");
        }
        if self.rotation.is_some_and(|value| !valid_angle(value)) {
            return Err("chat_background.rotation must be between -360 and 360 degrees");
        }
        if self.icon.is_some() && self.pattern != ChatPattern::Icon {
            return Err("chat_background.icon requires pattern = 'icon'");
        }
        match (self.pattern, self.svg.as_deref()) {
            (ChatPattern::Svg, Some(source)) if !has_asset => {
                self.svg =
                    Some(sanitize_svg(source.as_bytes(), SvgUse::ChatTile).ok_or(
                        "chat_background.svg must be a safe, static SVG tile with a viewBox",
                    )?);
            }
            (ChatPattern::Svg, None) if has_asset => {}
            (ChatPattern::Svg, None) => {
                return Err("chat_background.svg is required for pattern = 'svg'")
            }
            (_, Some(_)) => return Err("chat_background.svg requires pattern = 'svg'"),
            _ => {}
        }
        if self.pattern == ChatPattern::Image && !has_asset {
            return Err("chat_background.image requires a stored wallpaper asset");
        }
        if let Some(gradient) = &self.gradient {
            gradient.validate()?;
        }
        Ok(())
    }
}

impl ChatGradient {
    fn validate(&self) -> Result<(), &'static str> {
        if !(2..=4).contains(&self.stops.len()) || !self.stops.iter().all(|stop| hex_color(stop)) {
            return Err("chat_background.gradient.stops must contain two to four #rrggbb colors");
        }
        if self.angle.is_some_and(|angle| !valid_angle(angle)) {
            return Err("chat_background.gradient.angle must be between -360 and 360 degrees");
        }
        Ok(())
    }
}

fn valid_angle(angle: f64) -> bool {
    (-360.0..=360.0).contains(&angle)
}

fn hex_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests;
