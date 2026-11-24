use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{chat_background, ChatBackground};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "chat_background::deserialize"
    )]
    #[ts(optional)]
    pub chat_background: Option<ChatBackground>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "github-dark".to_string(),
            background: "#0d1117".to_string(),
            foreground: "#c9d1d9".to_string(),
            cursor: "#58a6ff".to_string(),
            selection: "#264f78".to_string(),
            black: "#0d1117".to_string(),
            red: "#ff7b72".to_string(),
            green: "#7ee787".to_string(),
            yellow: "#d29922".to_string(),
            blue: "#58a6ff".to_string(),
            magenta: "#bc8cff".to_string(),
            cyan: "#39d353".to_string(),
            white: "#c9d1d9".to_string(),
            bright_black: "#545d67".to_string(),
            bright_red: "#ffa198".to_string(),
            bright_green: "#56d364".to_string(),
            bright_yellow: "#e3b341".to_string(),
            bright_blue: "#79c0ff".to_string(),
            bright_magenta: "#d2a8ff".to_string(),
            bright_cyan: "#56d364".to_string(),
            bright_white: "#f0f6fc".to_string(),
            chat_background: None,
        }
    }
}
