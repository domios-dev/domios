use super::*;

#[test]
fn debug_frontend_build_has_no_instance_environment_inputs() {
    assert_eq!(
        directives("debug", |_| panic!(
            "debug compilation read instance identity"
        )),
        vec![
            "cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN=",
            "cargo:rustc-env=SERVER_DEV_IDENTIFIER=",
        ]
    );
}

#[test]
fn release_frontend_directives_preserve_existing_inputs_and_values() {
    let mut reads = Vec::new();
    let output = directives("release", |key| {
        reads.push(key.to_string());
        Some(match key {
            "CODETERM_SERVER_DEV_FRONTEND_ORIGIN" => "http://127.0.0.1:1821".into(),
            "CODETERM_SERVER_DEV_IDENTIFIER" => "com.domios.dev.opaque".into(),
            _ => panic!("unexpected release input"),
        })
    });
    assert_eq!(
        reads,
        [
            "CODETERM_SERVER_DEV_FRONTEND_ORIGIN",
            "CODETERM_SERVER_DEV_IDENTIFIER"
        ]
    );
    assert_eq!(
        output,
        [
            "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_FRONTEND_ORIGIN",
            "cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN=http://127.0.0.1:1821",
            "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_IDENTIFIER",
            "cargo:rustc-env=SERVER_DEV_IDENTIFIER=com.domios.dev.opaque",
        ]
    );
    assert_eq!(
        directives("release", |_| None),
        [
            "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_FRONTEND_ORIGIN",
            "cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN=",
            "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_IDENTIFIER",
            "cargo:rustc-env=SERVER_DEV_IDENTIFIER=",
        ]
    );
}

#[test]
#[should_panic(expected = "Invalid development frontend origin")]
fn release_frontend_still_rejects_invalid_build_input() {
    directives("release", |_| Some("bad origin".into()));
}
