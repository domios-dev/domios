#[test]
fn plugin_identity_shared_rule_refuses_files_paths_and_device_names() {
    for id in [
        "",
        ".",
        "..",
        "../host",
        "/host",
        "C:/host",
        "host.name",
        "nul",
        "NUL",
        "con",
        "com1",
        "lpt9",
        "blueprint-plugin-operations.sqlite",
        "blueprint-run-media.sqlite",
    ] {
        assert!(!super::valid(id), "{id}");
    }
    for id in ["valid-plugin", "fixture_2", "provider-1"] {
        assert!(super::valid(id));
    }
}
