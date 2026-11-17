use std::path::PathBuf;

const ROOT_ENVIRONMENT: &str = "CLAUDE_CONFIG_DIR";

pub(super) fn config_root() -> Option<PathBuf> {
    if crate::provider_account::current().is_some() {
        if !crate::provider_account::environment_is_selected(ROOT_ENVIRONMENT) {
            return None;
        }
        return crate::provider_account::environment(ROOT_ENVIRONMENT).map(PathBuf::from);
    }
    select_root(
        crate::provider_account::environment(ROOT_ENVIRONMENT),
        dirs::home_dir(),
    )
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
