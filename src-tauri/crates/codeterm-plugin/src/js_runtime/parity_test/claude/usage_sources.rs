//! Claude parity for `additional_usage_sources`: native and the bundled JS plugin list
//! the same sub-agent logs beside one session file, and never another session's.

use std::path::{Path, PathBuf};

use super::super::*;
use super::*;

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "{}\n").unwrap();
}

#[test]
fn additional_usage_sources_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let tmp = tempfile::tempdir().unwrap();
    let session_id = "f44205ef-4a6f-4090-9506-1978b4eada1b";
    let session = tmp.path().join(format!("{session_id}.jsonl"));
    touch(&session);
    let own = tmp.path().join(session_id).join("subagents");
    touch(&own.join("agent-af66d7d8e4a99a6bb.jsonl"));
    touch(&own.join("agent-a1.jsonl"));
    touch(&own.join("agent-af66d7d8e4a99a6bb.meta.json"));
    touch(&own.join("notes.jsonl"));
    touch(
        &tmp.path()
            .join("foreign-session")
            .join("subagents")
            .join("agent-x.jsonl"),
    );

    let expected = vec![
        own.join("agent-a1.jsonl"),
        own.join("agent-af66d7d8e4a99a6bb.jsonl"),
    ];
    assert_eq!(
        native.additional_usage_sources(&session, session_id),
        expected
    );
    assert_eq!(js.additional_usage_sources(&session, session_id), expected);

    let absent = tmp.path().join("absent.jsonl");
    for (file, id) in [(&session, "foreign-session"), (&absent, "absent")] {
        assert_eq!(
            native.additional_usage_sources(file, id),
            Vec::<PathBuf>::new(),
            "native must read only the logs of the session the file is named for"
        );
        assert_eq!(
            js.additional_usage_sources(file, id),
            native.additional_usage_sources(file, id),
            "JS must agree with native for {id}"
        );
    }
}
