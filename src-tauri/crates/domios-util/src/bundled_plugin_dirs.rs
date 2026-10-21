pub const CORE_PLUGIN_DIRS: &[&str] = &["analytics", "browser"];

pub fn bundled_plugin_id(directory: &str) -> Option<&str> {
    directory
        .strip_prefix("provider-")
        .or_else(|| CORE_PLUGIN_DIRS.contains(&directory).then_some(directory))
}
