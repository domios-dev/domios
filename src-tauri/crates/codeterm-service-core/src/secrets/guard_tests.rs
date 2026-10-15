#[test]
fn plugin_bucket_name_matches_the_host_api_convention() {
    let src = include_str!("../service/plugin_host.rs");
    assert!(
        src.contains("crate::secrets::plugin_secret_bucket"),
        "plugin_host must resolve its bucket through the shared helper"
    );
}
