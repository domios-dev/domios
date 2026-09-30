use super::*;

fn with_scratch_root(body: impl FnOnce(&std::path::Path)) {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("CODETERM_DATA_DIR").ok();
    let root = std::env::temp_dir().join(format!("codeterm-plugin-test-{}", uuid::Uuid::new_v4()));
    std::env::set_var("CODETERM_DATA_DIR", &root);
    body(&root);
    match prev {
        Some(value) => std::env::set_var("CODETERM_DATA_DIR", value),
        None => std::env::remove_var("CODETERM_DATA_DIR"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_of_an_unknown_plugin_says_it_is_not_installed() {
    with_scratch_root(|_| {
        let error = update_one("nosuch").unwrap_err();
        assert!(error.contains("nosuch"), "{error}");
        assert!(error.contains("not installed"), "{error}");
        assert!(!error.contains(".codeterm-install.json"), "{error}");
    });
}

#[test]
fn channel_add_without_a_channel_manifest_names_the_file_and_leaves_no_cache() {
    with_scratch_root(|root| {
        let source = root.join("empty-source");
        std::fs::create_dir_all(&source).unwrap();
        let error =
            add_channel(&source.display().to_string(), Some("qa-local".into())).unwrap_err();
        assert!(error.contains("channel.json"), "{error}");
        assert!(
            !channels_root().join("qa-local").exists(),
            "a failed add must not leave an unregistered cache directory"
        );
    });
}

#[test]
fn get_of_a_bundled_system_plugin_is_not_reported_as_not_installed() {
    with_scratch_root(|_| {
        let error = config::get_command("claude").unwrap_err();
        assert!(!error.contains("is not installed"), "{error}");
        assert!(error.contains("bundled"), "{error}");
        let unknown = config::get_command("nosuch").unwrap_err();
        assert!(unknown.contains("is not installed"), "{unknown}");
    });
}
