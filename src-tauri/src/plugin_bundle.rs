use include_dir::{include_dir, Dir};

include!(concat!(env!("OUT_DIR"), "/system_plugin_hashes.rs"));
static FILES: Dir<'static> = include_dir!("$OUT_DIR/bundled_plugins");

pub fn install() {
    codeterm_plugin::bundled::install(SYSTEM_PLUGIN_HASHES, &FILES);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_bundle_hashes_match_extracted_bytes() {
        install();
        let tmp = tempfile::tempdir().unwrap();
        FILES.extract(tmp.path()).unwrap();
        assert!(!SYSTEM_PLUGIN_HASHES.is_empty());
        for (id, expected) in SYSTEM_PLUGIN_HASHES {
            assert_eq!(
                codeterm_plugin::bundle_hash::hash_bundle(&tmp.path().join(id)).unwrap(),
                *expected
            );
            assert_eq!(codeterm_plugin::system_plugins::lookup(id), Some(*expected));
        }
    }
}
