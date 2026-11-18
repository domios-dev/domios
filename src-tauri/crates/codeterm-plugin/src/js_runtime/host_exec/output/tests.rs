use super::*;
use crate::js_runtime::{
    host_exec,
    vm::{call_json, PluginVm},
};
use serde_json::{json, Value};

#[test]
fn oversized_fake_exec_result_reaches_plugin_with_size_and_limit() {
    let plugin = "oversized-exec-output";
    let payload = "λ".repeat(LIMIT);
    let output = result(
        0,
        capture(&mut payload.as_bytes()),
        Capture::default(),
        None,
    );
    let job = host_exec::start_exec_job(plugin).unwrap();
    host_exec::finish_exec_job(&job, &json!(output));
    let vm = PluginVm::from_source(
        plugin,
        r#"module.exports = {
        read(job) { return host.awaitJob(job, function(result) { return result; }); }
    };"#
        .into(),
    )
    .unwrap();
    let reply = call_json::<Value>(&vm, "read", &[json!(job)]).unwrap();
    assert_eq!(
        reply["outputTooLarge"],
        json!({
            "stream":"stdout", "limitBytes":131072, "actualBytes":262144
        })
    );
    assert!(reply["error"]
        .as_str()
        .unwrap()
        .starts_with("output_too_large:"));
    assert_eq!(host_exec::plugin_job_count(plugin), 0);
}

#[test]
fn output_bound_counts_bytes_and_accepts_reported_dialog_size() {
    for bytes in [85_854, LIMIT, LIMIT + 1] {
        let payload = vec![b'x'; bytes];
        let output = result(
            0,
            capture(&mut payload.as_slice()),
            Capture::default(),
            None,
        );
        assert_eq!(output.stdout.unwrap().len(), bytes.min(LIMIT));
        assert_eq!(output.output_too_large.is_some(), bytes > LIMIT);
        assert_eq!(output.error.is_some(), bytes > LIMIT);
    }
    let payload = vec![b'x'; LIMIT + 9];
    let output = result(
        0,
        Capture::default(),
        capture(&mut payload.as_slice()),
        None,
    );
    assert_eq!(output.output_too_large.unwrap().stream, "stderr");
}
