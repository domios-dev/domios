use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{AppConfig, ChatWallpaper, ThemeConfig};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "RawTuning")]
pub struct AppearanceTuning {
    pub roundness: f64,
    pub shadow_strength: f64,
    pub border_strength: f64,
    pub font_size: i8,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyEdges {
    Subtle,
    Defined,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTuning {
    roundness: f64,
    shadow_strength: f64,
    border_strength: Option<f64>,
    #[serde(rename = "edges")]
    legacy_edges: Option<LegacyEdges>,
    font_size: i8,
}

impl Default for RawTuning {
    fn default() -> Self {
        let tuning = AppearanceTuning::default();
        Self {
            roundness: tuning.roundness,
            shadow_strength: tuning.shadow_strength,
            border_strength: None,
            legacy_edges: None,
            font_size: tuning.font_size,
        }
    }
}

impl Default for AppearanceTuning {
    fn default() -> Self {
        Self {
            roundness: 1.0,
            shadow_strength: 1.0,
            border_strength: 0.25,
            font_size: 0,
        }
    }
}

impl TryFrom<RawTuning> for AppearanceTuning {
    type Error = &'static str;

    fn try_from(raw: RawTuning) -> Result<Self, Self::Error> {
        let tuning = Self {
            roundness: raw.roundness,
            shadow_strength: raw.shadow_strength,
            border_strength: raw
                .border_strength
                .unwrap_or_else(|| match raw.legacy_edges {
                    Some(LegacyEdges::Defined) => 1.0,
                    _ => Self::default().border_strength,
                }),
            font_size: raw.font_size,
        };
        tuning.validate()?;
        Ok(tuning)
    }
}

impl AppearanceTuning {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(0.0..=1.0).contains(&self.roundness) {
            return Err("appearance.roundness must be between 0 and 1");
        }
        if !(0.0..=1.0).contains(&self.shadow_strength) {
            return Err("appearance.shadow_strength must be between 0 and 1");
        }
        if !(0.0..=1.0).contains(&self.border_strength) {
            return Err("appearance.border_strength must be between 0 and 1");
        }
        if !(-1..=2).contains(&self.font_size) {
            return Err("appearance.font_size must be an integer between -1 and 2");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceLook {
    pub theme: ThemeConfig,
    pub wallpaper: Option<ChatWallpaper>,
    pub tuning: Option<AppearanceTuning>,
}

impl AppearanceLook {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            theme: config.theme.clone(),
            wallpaper: config.chat_wallpaper.clone(),
            tuning: config.appearance,
        }
    }

    pub fn write_to(&self, config: &mut AppConfig) {
        config.theme = self.theme.clone();
        config.chat_wallpaper.clone_from(&self.wallpaper);
        config.appearance = self.tuning;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceState {
    Proposed,
    Applied,
    Undone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceProposal {
    pub id: String,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_by_installation: Option<String>,
    #[serde(default)]
    pub tweak: bool,
    pub look: AppearanceLook,
    pub previous: AppearanceLook,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_active: Option<String>,
    pub state: AppearanceState,
    pub applied_at: Option<u64>,
}
