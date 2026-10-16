use super::*;

pub(super) fn text_sibling(provider: &JsProvider) -> JsProvider {
    let mut manifest = provider.manifest().clone();
    manifest.capabilities.record_sources.clear();
    let source = std::fs::read_to_string(manifest.entry_path().unwrap()).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        &manifest.id,
        source,
        Some(serde_json::to_string(&manifest).unwrap()),
    )
    .unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

fn jsonl_files(directory: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(jsonl_files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            files.push(path);
        }
    }
    files.sort();
    files
}

#[test]
fn bundled_text_and_kernel_columns_match_every_transcript_fixture_and_offset() {
    require_plugins().expect("bundled record fixtures are required");
    let mut cases = 0;
    for entry in std::fs::read_dir(workspace_plugins_root()).unwrap() {
        let directory = entry.unwrap().path();
        let path = directory.join("plugin.json");
        if !path.exists() {
            continue;
        }
        let manifest = super::super::PluginManifest::load(&path).unwrap();
        if !manifest
            .capabilities
            .record_sources
            .contains_key("sessionTranscript")
        {
            continue;
        }
        let kernel = load_one(&path).unwrap();
        let text = text_sibling(&kernel);
        for fixture in jsonl_files(&directory.join("tests/fixtures")) {
            let mut bytes = std::fs::read(&fixture).unwrap();
            if bytes.last() != Some(&b'\n') {
                bytes.push(b'\n');
            }
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), &bytes).unwrap();
            for budget in [1, 1024, crate::jsonl_window::DELTA_CHUNK_BYTES] {
                let mut offset = 0;
                while offset < bytes.len() as u64 {
                    let old = text
                        .parse_session_delta_bounded(file.path(), offset, budget)
                        .unwrap();
                    let new = kernel
                        .parse_session_delta_bounded(file.path(), offset, budget)
                        .unwrap();
                    assert_eq!(
                        serde_json::to_value(&new).unwrap(),
                        serde_json::to_value(&old).unwrap(),
                        "{} at {offset}",
                        fixture.display()
                    );
                    assert!(
                        new.new_offset > offset,
                        "{} must advance",
                        fixture.display()
                    );
                    offset = new.new_offset;
                }
            }
            assert_eq!(
                serde_json::to_value(kernel.parse_session_messages(file.path())).unwrap(),
                serde_json::to_value(text.parse_session_messages(file.path())).unwrap(),
                "{} search",
                fixture.display()
            );
            cases += 1;
        }
    }
    assert!(
        cases >= 10,
        "both bundled transcript fixture sets must be covered"
    );
}

#[test]
fn bundled_captured_runtime_records_match_text_sibling_after_host_filter() {
    let mut cases = 0;
    for entry in std::fs::read_dir(workspace_plugins_root()).unwrap() {
        let directory = entry.unwrap().path();
        let path = directory.join("plugin.json");
        if !path.exists() {
            continue;
        }
        let manifest = super::super::PluginManifest::load(&path).unwrap();
        let Some(source) = manifest.capabilities.record_sources.get("storedHistory") else {
            continue;
        };
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                directory.join("tests/fixtures/recorded-runtime-history.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let session = fixture["sessionId"].as_str().unwrap();
        let input = fixture["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record.to_string() + "\n")
            .collect::<String>();
        let kernel = load_one(&path).unwrap();
        let text = text_sibling(&kernel);
        let window = crate::jsonl_window::records::parse_records_with_limit(
            input.as_bytes(),
            37,
            source,
            Some(session),
            1024 * 1024,
        );
        let expected = text.runtime_stored_history(session, &input).unwrap();
        let actual = kernel.runtime_stored_records(session, &window).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        cases += 1;
    }
    assert!(cases > 0, "a declared stored-record source is required");
}
