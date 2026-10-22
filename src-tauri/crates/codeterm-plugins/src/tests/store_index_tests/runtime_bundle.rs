use super::*;

#[test]
fn runtime_bundle_embedded_and_channel_installs_have_identical_bytes() {
    with_data_root(|root| {
        let source = root.join("channel");
        let embedded = root.join("embedded");
        std::fs::create_dir_all(source.join("tests")).unwrap();
        std::fs::create_dir_all(source.join("fixtures")).unwrap();
        std::fs::create_dir_all(source.join("src")).unwrap();
        for (name, body) in [
            (
                "runtime-files.txt",
                "plugin.json\nplugin.js\nruntime-files.txt\n",
            ),
            ("plugin.json", r#"{ "id": "demo", "entry": "plugin.js" }"#),
            ("plugin.js", "runtime"),
            ("tests/test.js", "test"),
            ("fixtures/data.json", "{}"),
            ("src/plugin.ts", "source"),
        ] {
            std::fs::write(source.join(name), body).unwrap();
        }
        codeterm_util::plugin_runtime_files::copy_runtime_bundle(&source, &embedded).unwrap();
        install_from_dir(&embedded, "demo", None).unwrap();
        let installed = root.join("plugins/demo");
        let snapshot = |directory: &Path| {
            let mut entries: Vec<_> = std::fs::read_dir(directory)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), std::fs::read(entry.path()).unwrap())
                })
                .collect();
            entries.sort();
            entries
        };
        let embedded_install = snapshot(&installed);
        std::fs::remove_dir_all(&installed).unwrap();
        install_from_dir(&source, "demo", None).unwrap();
        assert_eq!(snapshot(&installed), embedded_install);
        assert_eq!(snapshot(&installed), snapshot(&embedded));
    });
}
