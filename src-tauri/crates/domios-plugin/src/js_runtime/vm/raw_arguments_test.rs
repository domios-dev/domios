use super::*;

#[test]
fn session_parser_argument_channels_preserve_rows() {
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut cases = 0;
    for entry in std::fs::read_dir(plugins).unwrap() {
        let directory = entry.unwrap().path();
        let path = directory.join("plugin.json");
        let Ok(manifest) = crate::js_runtime::PluginManifest::load(&path) else {
            continue;
        };
        if !manifest
            .capabilities
            .record_sources
            .contains_key("sessionTranscript")
        {
            continue;
        }
        let vm = PluginVm::from_source(
            &manifest.id,
            std::fs::read_to_string(manifest.entry_path().unwrap()).unwrap(),
        )
        .unwrap();
        for entry in std::fs::read_dir(directory.join("tests/fixtures")).unwrap() {
            let path = entry.unwrap().path();
            if !path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                continue;
            }
            let records = std::fs::read_to_string(&path).unwrap();
            let args = [serde_json::json!(records)];
            for method in ["parseSessionDelta", "parseSessionMessages"] {
                let json = Arguments {
                    json: serde_json::to_string(&args).unwrap(),
                    raw: None,
                    objects: None,
                    window: None,
                };
                let raw = Arguments::prepare(method, &args).unwrap();
                assert!(raw.raw.is_some());
                let expected = enter_call(&vm, method, &json, false, None, None).unwrap();
                let actual = enter_call(&vm, method, &raw, false, None, None).unwrap();
                assert_eq!(actual, expected, "{} {method}", path.display());
                cases += 1;
            }
        }
    }
    assert!(
        cases >= 12,
        "bundled fixtures must exercise both text channels"
    );
}

#[test]
fn stored_history_argument_measurement() {
    let vm = PluginVm::from_source(
        "argument-measurement",
        "module.exports = { runtimeStoredHistory(session, records) { return records.length; } };"
            .into(),
    )
    .unwrap();
    let mut records =
        "{\"text\":\"quotes \\\" and slash \\\\ and escaped surrogate \\ud800\"}\n".repeat(20_000);
    records.truncate(1_100_000);
    let args = [serde_json::json!("session"), serde_json::json!(records)];
    for raw in [false, true] {
        let mut prepare = std::time::Duration::ZERO;
        let mut execute = std::time::Duration::ZERO;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let prepared = if raw {
                Arguments::prepare("runtimeStoredHistory", &args).unwrap()
            } else {
                Arguments {
                    json: serde_json::to_string(&args).unwrap(),
                    raw: None,
                    objects: None,
                    window: None,
                }
            };
            prepare += start.elapsed();
            let start = std::time::Instant::now();
            let output =
                enter_call(&vm, "runtimeStoredHistory", &prepared, false, None, None).unwrap();
            execute += start.elapsed();
            assert_eq!(output.parse::<usize>().unwrap(), records.len());
        }
        eprintln!(
        "argument-channel raw={raw} bytes={} iterations=5 prepare_mean_us={} execute_mean_us={}",
        records.len(), prepare.as_micros() / 5, execute.as_micros() / 5
    );
    }
}
