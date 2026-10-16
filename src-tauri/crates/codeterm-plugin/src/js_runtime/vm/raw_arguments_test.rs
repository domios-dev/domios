use super::*;

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
