use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_blueprint_types::sentinel_input::MediaKind;

use super::*;

fn kind(s: &str) -> MediaKind {
    MediaKind::parse(s).expect("fixture kind")
}

/// Fake converter whose only identity is its declared edges; `convert` names
/// the fixture so a step's chosen converter is observable in the output.
struct FakeConverter {
    name: &'static str,
    edges: Vec<Conversion>,
}

impl FakeConverter {
    fn new(name: &'static str, edges: &[(&str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            name,
            edges: edges
                .iter()
                .map(|(from, to)| Conversion::new(kind(from), kind(to)))
                .collect(),
        })
    }
}

impl Converter for FakeConverter {
    fn conversions(&self) -> &[Conversion] {
        &self.edges
    }

    fn convert(&self, req: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        Ok(ConvertOutput::Text(format!(
            "{}:{}",
            self.name,
            req.to.as_str()
        )))
    }
}

fn request(from: &str, to: &str) -> ConvertRequest {
    ConvertRequest {
        input_path: PathBuf::from("in.bin"),
        from: kind(from),
        to: kind(to),
        output_dir: PathBuf::from("out"),
        language: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancellation: Default::default(),
    }
}

/// The chain's edges, in execution order.
fn shape(plan: &[Step]) -> Vec<(MediaKind, MediaKind)> {
    plan.iter()
        .map(|step| match step {
            Step::Identity => (MediaKind::Text, MediaKind::Text),
            Step::Convert { conversion, .. } => (conversion.from, conversion.to),
        })
        .collect()
}

fn edge(from: &str, to: &str) -> (MediaKind, MediaKind) {
    (kind(from), kind(to))
}

#[test]
fn plan_conversion_identity_for_text_to_text() {
    let plan = plan_conversion(&[], MediaKind::Text, MediaKind::Text).expect("identity");
    assert_eq!(plan, vec![Step::Identity]);
}

#[test]
fn plan_conversion_direct_converter_when_one_exists() {
    let audio = FakeConverter::new("audio-fixture", &[("video", "audio")]);
    let available = vec![(
        ConverterKey::new(audio.clone()),
        Conversion::new(kind("video"), kind("audio")),
    )];
    let plan = plan_conversion(&available, kind("video"), kind("audio")).expect("direct");
    assert_eq!(shape(&plan), vec![edge("video", "audio")]);
    assert!(matches!(
        &plan[0],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(audio.clone()))
    ));
}

#[test]
fn plan_conversion_two_step_chain_video_audio_text() {
    let extract = FakeConverter::new("extract-fixture", &[("video", "audio")]);
    let speech = FakeConverter::new("speech-fixture", &[("audio", "text")]);
    let available = vec![
        (
            ConverterKey::new(extract.clone()),
            Conversion::new(kind("video"), kind("audio")),
        ),
        (
            ConverterKey::new(speech.clone()),
            Conversion::new(kind("audio"), kind("text")),
        ),
    ];
    let plan = plan_conversion(&available, kind("video"), kind("text")).expect("chain");
    assert_eq!(
        shape(&plan),
        vec![edge("video", "audio"), edge("audio", "text")]
    );
    assert!(matches!(
        &plan[0],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(extract.clone()))
    ));
    assert!(matches!(
        &plan[1],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(speech.clone()))
    ));
}

#[test]
fn plan_conversion_three_step_chain_is_planned() {
    let frames = FakeConverter::new("frames-fixture", &[("image", "video")]);
    let extract = FakeConverter::new("extract-fixture", &[("video", "audio")]);
    let speech = FakeConverter::new("speech-fixture", &[("audio", "text")]);
    let available = vec![
        (
            ConverterKey::new(frames),
            Conversion::new(kind("image"), kind("video")),
        ),
        (
            ConverterKey::new(extract),
            Conversion::new(kind("video"), kind("audio")),
        ),
        (
            ConverterKey::new(speech),
            Conversion::new(kind("audio"), kind("text")),
        ),
    ];
    let plan = plan_conversion(&available, kind("image"), kind("text")).expect("three-step chain");
    assert_eq!(
        shape(&plan),
        vec![
            edge("image", "video"),
            edge("video", "audio"),
            edge("audio", "text")
        ]
    );
}

#[test]
fn plan_conversion_no_converter_names_the_missing_pair() {
    let extract = FakeConverter::new("extract-fixture", &[("video", "audio")]);
    let available = vec![(
        ConverterKey::new(extract),
        Conversion::new(kind("video"), kind("audio")),
    )];
    let err = plan_conversion(&available, kind("video"), kind("text")).expect_err("no chain");
    assert_eq!(
        err,
        NoConverter {
            from: kind("video"),
            to: kind("text"),
        }
    );
    let rendered = err.to_string();
    assert!(rendered.contains("video"), "{rendered}");
    assert!(rendered.contains("text"), "{rendered}");
}

#[test]
fn plan_conversion_direct_edge_beats_longer_chain() {
    let direct = FakeConverter::new("direct-fixture", &[("video", "text")]);
    let extract = FakeConverter::new("extract-fixture", &[("video", "audio")]);
    let speech = FakeConverter::new("speech-fixture", &[("audio", "text")]);
    let available = vec![
        (
            ConverterKey::new(extract),
            Conversion::new(kind("video"), kind("audio")),
        ),
        (
            ConverterKey::new(speech),
            Conversion::new(kind("audio"), kind("text")),
        ),
        (
            ConverterKey::new(direct.clone()),
            Conversion::new(kind("video"), kind("text")),
        ),
    ];
    let plan = plan_conversion(&available, kind("video"), kind("text")).expect("direct wins");
    assert_eq!(shape(&plan), vec![edge("video", "text")]);
    assert!(matches!(
        &plan[0],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(direct.clone()))
    ));
}

#[test]
fn plan_conversion_ties_resolve_by_available_order() {
    let first = FakeConverter::new("first-fixture", &[("audio", "text")]);
    let second = FakeConverter::new("second-fixture", &[("audio", "text")]);
    let available = vec![
        (
            ConverterKey::new(first.clone()),
            Conversion::new(kind("audio"), kind("text")),
        ),
        (
            ConverterKey::new(second.clone()),
            Conversion::new(kind("audio"), kind("text")),
        ),
    ];
    let plan = plan_conversion(&available, kind("audio"), kind("text")).expect("first wins");
    assert!(matches!(
        &plan[0],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(first.clone()))
    ));

    let reversed = vec![available[1].clone(), available[0].clone()];
    let plan = plan_conversion(&reversed, kind("audio"), kind("text")).expect("reversed wins");
    assert!(matches!(
        &plan[0],
        Step::Convert { key, .. } if key.same_as(&ConverterKey::new(second.clone()))
    ));
}

#[test]
fn plan_conversion_depends_on_edges_not_converter_names() {
    let renamed_a = FakeConverter::new("alpha-fixture", &[("video", "audio")]);
    let renamed_b = FakeConverter::new("beta-fixture", &[("audio", "text")]);
    let named_x = FakeConverter::new("x-ray-fixture", &[("video", "audio")]);
    let named_y = FakeConverter::new("yankee-fixture", &[("audio", "text")]);
    let before = vec![
        (
            ConverterKey::new(renamed_a),
            Conversion::new(kind("video"), kind("audio")),
        ),
        (
            ConverterKey::new(renamed_b),
            Conversion::new(kind("audio"), kind("text")),
        ),
    ];
    let after = vec![
        (
            ConverterKey::new(named_x),
            Conversion::new(kind("video"), kind("audio")),
        ),
        (
            ConverterKey::new(named_y),
            Conversion::new(kind("audio"), kind("text")),
        ),
    ];
    assert_eq!(
        shape(&plan_conversion(&before, kind("video"), kind("text")).unwrap()),
        shape(&plan_conversion(&after, kind("video"), kind("text")).unwrap())
    );
}

#[test]
fn plan_conversion_non_text_identity_is_not_planned() {
    let err = plan_conversion(&[], MediaKind::Audio, MediaKind::Audio)
        .expect_err("only text converts to itself");
    assert_eq!(
        err,
        NoConverter {
            from: MediaKind::Audio,
            to: MediaKind::Audio,
        }
    );
}

// --- registry ---

#[test]
fn converter_registry_flattens_edges_in_registration_order() {
    let registry = ConverterRegistry::new();
    let extract = FakeConverter::new("extract-fixture", &[("video", "audio"), ("image", "audio")]);
    let speech = FakeConverter::new("speech-fixture", &[("audio", "text")]);
    registry.register("extract-fixture", extract.clone());
    registry.register("speech-fixture", speech.clone());

    let available = registry.available();
    assert_eq!(
        available
            .iter()
            .map(|(_, c)| *c)
            .collect::<Vec<Conversion>>(),
        vec![
            Conversion::new(kind("video"), kind("audio")),
            Conversion::new(kind("image"), kind("audio")),
            Conversion::new(kind("audio"), kind("text")),
        ]
    );
    let plan = registry.plan(kind("video"), kind("text")).expect("chain");
    assert_eq!(
        shape(&plan),
        vec![edge("video", "audio"), edge("audio", "text")]
    );
}

#[test]
fn converter_registry_accumulates_per_id_and_unregister_removes_all() {
    let registry = ConverterRegistry::new();
    let first = FakeConverter::new("first-fixture", &[("video", "audio")]);
    let second = FakeConverter::new("second-fixture", &[("image", "text")]);
    registry.register("slot", first.clone());
    registry.register("other", second.clone());
    let derived = FakeConverter::new("derived-fixture", &[("image", "video")]);
    registry.register("slot", derived.clone());

    let available = registry.available();
    assert_eq!(
        available
            .iter()
            .map(|(_, c)| *c)
            .collect::<Vec<Conversion>>(),
        vec![
            Conversion::new(kind("video"), kind("audio")),
            Conversion::new(kind("image"), kind("video")),
            Conversion::new(kind("image"), kind("text")),
        ],
        "one id may contribute several converters; the id keeps its slot"
    );
    assert_eq!(registry.converters_for("slot").len(), 2);
    assert!(registry.plan(kind("video"), kind("audio")).is_ok());
    assert!(registry.plan(kind("image"), kind("video")).is_ok());

    registry.unregister("slot");
    assert!(registry.converters_for("slot").is_empty());
    assert_eq!(registry.available().len(), 1);
    assert!(registry.plan(kind("video"), kind("audio")).is_err());
    assert!(registry.plan(kind("image"), kind("video")).is_err());
}

// --- transcriber adapter ---

struct EchoTranscriber {
    text: String,
}

#[async_trait::async_trait]
impl Transcriber for EchoTranscriber {
    fn id(&self) -> &str {
        "echo-fixture"
    }

    async fn status(&self) -> crate::transcribe::TranscriberStatus {
        crate::transcribe::TranscriberStatus::Ready
    }

    async fn transcribe(
        &self,
        audio_path: &std::path::Path,
        language: &str,
    ) -> Result<crate::transcribe::Transcript, TranscribeError> {
        if self.text.is_empty() {
            return Err(TranscribeError::RequestFailed("fixture down".into()));
        }
        Ok(crate::transcribe::Transcript {
            text: format!("{}|{}|{}", self.text, audio_path.display(), language),
        })
    }
}

#[test]
fn transcriber_adapter_converts_audio_to_text() {
    let adapter = TranscriberConverter::new(Arc::new(EchoTranscriber {
        text: "heard".into(),
    }));
    assert_eq!(
        adapter.conversions(),
        &[Conversion::new(MediaKind::Audio, MediaKind::Text)]
    );
    let mut req = request("audio", "text");
    req.input_path = PathBuf::from("clip.ova");
    req.language = Some("ru".into());
    match adapter.convert(&req).expect("audio to text") {
        ConvertOutput::Text(text) => assert_eq!(text, "heard|clip.ova|ru"),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn transcriber_adapter_maps_failures_and_undeclared_pairs() {
    let adapter = TranscriberConverter::new(Arc::new(EchoTranscriber {
        text: String::new(),
    }));
    assert!(matches!(
        adapter.convert(&request("audio", "text")),
        Err(ConvertError::RequestFailed(_))
    ));
    assert!(matches!(
        adapter.convert(&request("image", "text")),
        Err(ConvertError::InvalidRequest(_))
    ));

    let mut expired = request("audio", "text");
    expired.deadline = Instant::now();
    assert!(matches!(
        adapter.convert(&expired),
        Err(ConvertError::DeadlineExceeded)
    ));
}

// --- output containment ---

#[test]
fn output_within_refuses_paths_outside_the_output_dir() {
    let dir = std::path::Path::new("out");
    assert!(output_within(dir, &PathBuf::from("out/result.txt")));
    assert!(output_within(dir, &PathBuf::from("out/nested/result.txt")));
    assert!(!output_within(dir, &PathBuf::from("out")));
    assert!(!output_within(dir, &PathBuf::from("elsewhere/result.txt")));
    assert!(!output_within(dir, &PathBuf::from("out/../escape.txt")));
    assert!(!output_within(dir, &PathBuf::from("out/./../escape.txt")));
}
