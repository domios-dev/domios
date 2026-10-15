use std::sync::OnceLock;

static INFO: OnceLock<(&'static str, &'static str)> = OnceLock::new();

/// The root package installs the commit it was built from; a leaf crate embedding it would rebuild every dependent per commit.
pub fn install(sha: &'static str, hash: &'static str) {
    let _ = INFO.set((sha, hash));
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
}
