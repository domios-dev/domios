use std::time::{Duration, Instant};

use codeterm_types::sentinel_input::MediaKind;

use super::*;
use crate::converter::{Conversion, ConvertOutput, ConvertRequest};

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("js-converter-fixture", src.to_string()).expect("vm"))
}

fn converter(vm: &Arc<PluginVm>) -> JsConverter {
    JsConverter::new(
        vm.clone(),
        vec![Conversion::new(MediaKind::Audio, MediaKind::Text)],
    )
}

fn request(output_dir: &std::path::Path) -> ConvertRequest {
    ConvertRequest {
        input_path: output_dir.join("clip.ova"),
        from: MediaKind::Audio,
        to: MediaKind::Text,
        output_dir: output_dir.to_path_buf(),
        language: Some("ru".into()),
        deadline: Instant::now() + Duration::from_secs(30),
        cancellation: Default::default(),
    }
}

#[test]
fn js_converter_maps_text() {
    let vm = vm_with(
        r#"module.exports.default = {
            convert(request) {
                return { text: request.from + "->" + request.to + ":" + request.language };
            }
        };"#,
    );
    let dir = tempfile::tempdir().unwrap();
    match converter(&vm).convert(&request(dir.path())).expect("text") {
        ConvertOutput::Text(text) => assert_eq!(text, "audio->text:ru"),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn js_converter_maps_a_path_inside_the_output_dir() {
    let vm = vm_with(
        r#"module.exports.default = {
            convert(request) { return { path: request.outputDir + "/out.txt" }; }
        };"#,
    );
    let dir = tempfile::tempdir().unwrap();
    let expected = dir.path().join("out.txt");
    match converter(&vm).convert(&request(dir.path())).expect("file") {
        ConvertOutput::File(path) => assert_eq!(path, expected),
        other => panic!("expected a file, got {other:?}"),
    }
}

#[test]
fn js_converter_refuses_a_path_outside_the_output_dir() {
    for expression in [
        r#"request.outputDir + "/../escape.txt""#,
        r#"request.outputDir + "/nested/../../escape.txt""#,
        r#"request.outputDir + "-sibling/out.txt""#,
    ] {
        let vm = vm_with(&format!(
            r#"module.exports.default = {{
                convert(request) {{ return {{ path: {expression} }}; }}
            }};"#
        ));
        let dir = tempfile::tempdir().unwrap();
        let err = converter(&vm)
            .convert(&request(dir.path()))
            .expect_err("path outside output_dir must be refused");
        assert!(
            matches!(err, ConvertError::OutputOutsideDir(_)),
            "{expression}: {err:?}"
        );
    }
}

#[test]
fn js_converter_maps_error_and_missing_results() {
    let failing =
        vm_with(r#"module.exports.default = { convert() { return { error: "engine down" }; } };"#);
    let dir = tempfile::tempdir().unwrap();
    match converter(&failing).convert(&request(dir.path())) {
        Err(ConvertError::RequestFailed(message)) => assert_eq!(message, "engine down"),
        other => panic!("expected a request failure, got {other:?}"),
    }

    let empty = vm_with(r#"module.exports.default = { convert() { return {}; } };"#);
    assert!(matches!(
        converter(&empty).convert(&request(dir.path())),
        Err(ConvertError::NoOutput)
    ));

    let broken = vm_with(r#"module.exports.default = { convert() { throw new Error("boom"); } };"#);
    assert!(matches!(
        converter(&broken).convert(&request(dir.path())),
        Err(ConvertError::RequestFailed(_))
    ));
}

#[test]
fn js_converter_enforces_the_deadline_before_calling() {
    let vm = vm_with(r#"module.exports.default = { convert() { return { text: "late" }; } };"#);
    let dir = tempfile::tempdir().unwrap();
    let mut expired = request(dir.path());
    expired.deadline = Instant::now();
    assert!(matches!(
        converter(&vm).convert(&expired),
        Err(ConvertError::DeadlineExceeded)
    ));
}
