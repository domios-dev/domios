use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{call_failure, JsTranscriber};
use crate::js_runtime::call_gate;
use crate::js_runtime::vm::{call_json, PluginVm, VmError};
use crate::transcribe::{TranscribeError, Transcriber};

const PLUGIN: &str = "transcribe-deadline-fixture";
// A failure boundary for a wedged expectation, never evidence of success.
const WEDGED: Duration = Duration::from_secs(3);

fn source(transcription: &str, ordinary: &str) -> String {
    format!(
        r#"
module.exports.default = {{
  transcribe(path, language) {{
    return host.io({{op:"testGate",entered:"{transcription}.entered",release:"{transcription}.release",completed:"{transcription}.completed"}},
      function () {{ return {{ text: "fixture transcript " + language }}; }});
  }},
  ordinary() {{
    return host.io({{op:"testGate",entered:"{ordinary}.entered",release:"{ordinary}.release",completed:"{ordinary}.completed"}},
      function () {{ return true; }});
  }},
  ping() {{ return true; }},
}};
"#
    )
}

struct ReleaseOnDrop(Vec<String>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            call_gate::open(gate);
        }
    }
}

#[test]
fn slow_transcription_outlives_the_standard_bound_without_blocking_the_plugin() {
    let transcription = "transcribe-deadline.transcription";
    let ordinary = "transcribe-deadline.ordinary";
    for name in ["entered", "release", "completed"] {
        call_gate::reset(&format!("{transcription}.{name}"));
        call_gate::reset(&format!("{ordinary}.{name}"));
    }
    // The scaled-down standard bound stands in for the 5 s production bound.
    crate::js_runtime::host_exec::set_test_wait_bound(PLUGIN, Duration::from_millis(100));
    let manifest = json!({"id":PLUGIN,"version":"0.0.0","displayName":"Transcribe deadline","hostIoJobs":true});
    let vm = Arc::new(
        PluginVm::from_source_with(
            PLUGIN,
            source(transcription, ordinary),
            Some(manifest.to_string()),
        )
        .expect("vm"),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    // Declared after the runtime so a failed assertion releases the held jobs before the
    // runtime's drop waits for its blocking transcription.
    let _release = ReleaseOnDrop(vec![
        format!("{transcription}.release"),
        format!("{ordinary}.release"),
    ]);
    let transcriber = JsTranscriber::new(vm.clone(), PLUGIN);
    let pending =
        runtime.spawn(async move { transcriber.transcribe(Path::new("/note.webm"), "en").await });
    assert!(
        call_gate::wait_opened(&format!("{transcription}.entered"), WEDGED),
        "transcription never reached its host I/O"
    );

    let started = Instant::now();
    let timed_out = call_json::<bool>(&vm, "ordinary", &[]);
    let elapsed = started.elapsed();
    call_gate::open(&format!("{ordinary}.release"));
    // Closing a job at its deadline runs the job's cancel hook, which revokes the call.
    assert!(
        matches!(timed_out, Err(VmError::CallCancelled)) && elapsed < WEDGED,
        "an ordinary call must end at the standard bound: {timed_out:?} after {elapsed:?}"
    );
    assert!(
        call_json::<bool>(&vm, "ping", &[]).expect("a call between transcription frames"),
        "the plugin VM must stay free while transcription waits on host I/O"
    );
    let still_running = !pending.is_finished();
    call_gate::open(&format!("{transcription}.release"));
    assert!(
        still_running,
        "transcription ended before its host I/O was released"
    );
    let transcript = runtime
        .block_on(pending)
        .unwrap()
        .expect("the slow transcription must complete");
    assert_eq!(transcript.text, "fixture transcript en");
}

#[test]
fn interruption_past_the_deadline_is_an_honest_timeout() {
    let bound = Duration::from_secs(300);
    for error in [
        VmError::AsyncJobTimedOut,
        VmError::CallCancelled,
        VmError::IoUnconfirmed,
    ] {
        assert!(matches!(
            call_failure(error, true, bound),
            TranscribeError::TimedOut(reported) if reported == bound
        ));
    }
    assert!(matches!(
        call_failure(VmError::CallCancelled, false, bound),
        TranscribeError::RequestFailed(_)
    ));
    assert_eq!(
        TranscribeError::TimedOut(bound).to_string(),
        "transcription did not finish within 300 s"
    );
}
