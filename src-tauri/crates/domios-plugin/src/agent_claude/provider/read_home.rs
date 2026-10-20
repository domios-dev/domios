use std::path::PathBuf;

pub(super) fn config_root() -> Option<PathBuf> {
    select_root(std::env::var_os("CLAUDE_CONFIG_DIR"), dirs::home_dir())
}

fn select_root(inherited: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match inherited.filter(|path| !path.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => home.map(|home| home.join(".claude")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_home_diagnostic_native_read_root_honors_inherited_home() {
        assert_eq!(
            select_root(Some("foreign".into()), Some("default".into())),
            Some("foreign".into())
        );
        assert_eq!(
            select_root(Some("".into()), Some("default".into())),
            Some(PathBuf::from("default").join(".claude"))
        );
        assert_eq!(select_root(None, None), None);
    }
}
