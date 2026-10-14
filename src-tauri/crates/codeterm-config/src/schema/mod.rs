pub use codeterm_types::config::schema::*;

pub mod theme;
pub use theme::{get_any_theme, list_all_theme_ids, parse_builtin_theme, user_themes_dir};
