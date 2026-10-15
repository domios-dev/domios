use std::sync::OnceLock;

static INFO: OnceLock<(&'static str, &'static str)> = OnceLock::new();
static VERSION: OnceLock<&'static str> = OnceLock::new();

/// The root package installs the commit it was built from; a leaf crate embedding it would rebuild every dependent per commit.
pub fn install(sha: &'static str, hash: &'static str) {
    let _ = INFO.set((sha, hash));
}

/// The root package installs the app version; internal crates stay at 0.0.0 so a version bump rebuilds only the root.
pub fn install_version(version: &'static str) {
    let _ = VERSION.set(version);
}

/// None before the root installs it; compatibility checks (plugin minCodeterm) must skip rather than compare.
pub fn installed_version() -> Option<&'static str> {
    VERSION.get().copied()
}

pub fn version() -> &'static str {
    installed_version().unwrap_or("unknown")
}

pub fn sha() -> &'static str {
    INFO.get().map_or("unknown", |info| info.0)
}

pub fn hash() -> &'static str {
    INFO.get().map_or("unknown", |info| info.1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn install_is_read_back_and_first_install_wins() {
        super::install("0123456789abcdef", "0123456");
        super::install("ffffffffffffffff", "fffffff");
        assert_eq!(super::sha(), "0123456789abcdef");
        assert_eq!(super::hash(), "0123456");
    }

    #[test]
    fn version_is_read_back_and_first_install_wins() {
        super::install_version("9.8.7");
        super::install_version("1.2.3");
        assert_eq!(super::version(), "9.8.7");
    }
}
