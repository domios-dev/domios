use include_dir::Dir;
use std::sync::OnceLock;

pub struct BundledPlugins {
    pub hashes: &'static [(&'static str, &'static str)],
    pub files: &'static Dir<'static>,
}

static BUNDLED: OnceLock<BundledPlugins> = OnceLock::new();

pub fn install(hashes: &'static [(&'static str, &'static str)], files: &'static Dir<'static>) {
    BUNDLED.get_or_init(|| BundledPlugins { hashes, files });
}

pub fn installed() -> Result<&'static BundledPlugins, String> {
    #[cfg(any(test, feature = "test-support"))]
    BUNDLED.get_or_init(crate::bundle_test_support::fixtures);
    require_installed(&BUNDLED)
}

fn require_installed(bundled: &OnceLock<BundledPlugins>) -> Result<&BundledPlugins, String> {
    bundled.get().ok_or_else(|| {
        "bundled plugins are not installed; install the executable's bundle before loading plugins".into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_installation_is_required_before_loading() {
        let state = OnceLock::new();
        let error = crate::js_runtime::loader::load_one_with_installation(
            std::path::Path::new("missing/plugin.json"),
            require_installed(&state),
        )
        .err()
        .unwrap();
        assert!(error.contains("not installed"), "{error}");
        static FILES: Dir<'static> = Dir::new("", &[]);
        assert!(state
            .set(BundledPlugins {
                hashes: &[("fixture", "hash")],
                files: &FILES
            })
            .is_ok());
        assert_eq!(
            require_installed(&state).unwrap().hashes,
            &[("fixture", "hash")]
        );
    }
}
