//! Converter parity: a JS `convert` export and a native fake converter agree on
//! shared fixtures, so the drop-in path cannot drift from a native
//! implementation of the same capability.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_types::sentinel_input::MediaKind;

use super::converter::JsConverter;
use crate::converter::{Conversion, ConvertError, ConvertOutput, ConvertRequest, Converter};

/// Native fake of the fixture conversion: uppercase the input file's text into
/// the requested output form.
struct NativeFakeConverter {
    conversions: Vec<Conversion>,
}

impl NativeFakeConverter {
    fn new() -> Self {
        Self {
            conversions: vec![
                Conversion::new(MediaKind::Image, MediaKind::Text),
                Conversion::new(MediaKind::Video, MediaKind::Audio),
            ],
        }
    }
}

impl Converter for NativeFakeConverter {
    fn conversions(&self) -> &[Conversion] {
        &self.conversions
    }

    fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        let text = std::fs::read_to_string(&req.input_path)
            .map_err(|e| ConvertError::RequestFailed(e.to_string()))?
            .to_uppercase();
        if req.to == MediaKind::Text {
            return Ok(ConvertOutput::Text(text));
        }
        let out = req.output_dir.join("converted.bin");
        std::fs::write(&out, text).map_err(|e| ConvertError::RequestFailed(e.to_string()))?;
        Ok(ConvertOutput::File(out))
    }
}

const JS_FIXTURE: &str = r#"module.exports.default = {
    convert(request) {
        var text = String(host.fs.readFile(request.inputPath)).toUpperCase();
        if (request.to === "text") {
            return { text: text };
        }
        var out = request.outputDir + "/converted.bin";
        host.fs.writeFile(out, text);
        return { path: out };
    }
};"#;

fn fixture_request(
    input: &std::path::Path,
    dir: &std::path::Path,
    to: MediaKind,
) -> ConvertRequest {
    ConvertRequest {
        input_path: input.to_path_buf(),
        from: if to == MediaKind::Text {
            MediaKind::Image
        } else {
            MediaKind::Video
        },
        to,
        output_dir: dir.to_path_buf(),
        language: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancellation: Default::default(),
    }
}

fn assert_js_matches_native(request: &ConvertRequest) {
    let vm = Arc::new(
        super::PluginVm::from_source("parity-converter-fixture", JS_FIXTURE.into()).expect("vm"),
    );
    let js = JsConverter::new(vm, NativeFakeConverter::new().conversions().to_vec());
    let native = NativeFakeConverter::new();
    assert_eq!(js.conversions(), native.conversions());

    let from_js = js.convert(request).expect("js converts");
    let from_native = native.convert(request).expect("native converts");
    assert_eq!(from_js, from_native, "js and native agree on the fixture");
}

#[test]
fn parity_converter_js_matches_native_fake_on_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.png");
    std::fs::write(&input, "hello fixture").unwrap();
    let request = fixture_request(&input, tmp.path(), MediaKind::Text);
    assert_js_matches_native(&request);
    let native = NativeFakeConverter::new();
    assert_eq!(
        native.convert(&request).expect("native converts"),
        ConvertOutput::Text("HELLO FIXTURE".into())
    );
}

#[test]
fn parity_converter_js_matches_native_fake_on_file_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let input = tmp.path().join("source.mp4");
    std::fs::write(&input, "hello fixture").unwrap();
    let request = fixture_request(&input, &out_dir, MediaKind::Audio);
    assert_js_matches_native(&request);
    let native = NativeFakeConverter::new();
    match native.convert(&request).expect("native converts") {
        ConvertOutput::File(path) => {
            assert_eq!(path, out_dir.join("converted.bin"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "HELLO FIXTURE");
        }
        other => panic!("expected a file, got {other:?}"),
    }
}
