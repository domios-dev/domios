use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPricing,
    DecisionPrimitives, DecisionQuestion, DecisionQuestionsRequest, DecisionRequest,
    NamedDecisionQuestion, TokenMode,
};
use serde_json::json;

use crate::js_runtime::{JsDecisionModel, PluginVm};

struct NativeDecisionModel(TokenMode);

impl DecisionModel for NativeDecisionModel {
    fn supports_calibrated_choice_confidence(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        "native-fixture"
    }

    fn model_id(&self) -> Option<String> {
        Some("fixture-model".into())
    }

    fn pricing(&self) -> Option<DecisionPricing> {
        Some(DecisionPricing {
            display_name: "local fixture".into(),
            input_usd_per_million: 0.0,
            output_usd_per_million: 0.0,
        })
    }

    fn token_mode(&self) -> TokenMode {
        self.0
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: true,
        }
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        vec![DecisionModelInfo {
            id: "fixture-model".into(),
            display_name: "Fixture model".into(),
        }]
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        Ok(match &request.question {
            DecisionQuestion::Noul { .. } => DecisionAnswer::Noul { p: 0.8 },
            DecisionQuestion::Choice { .. } => DecisionAnswer::Choice {
                choice: "yes".into(),
                probabilities: BTreeMap::from([("no".into(), 0.2), ("yes".into(), 0.8)]),
                confidence: Some(0.8),
            },
            DecisionQuestion::Score { .. } => DecisionAnswer::Score {
                score: 2.0,
                probabilities: BTreeMap::from([
                    ("0".into(), 0.1),
                    ("1".into(), 0.2),
                    ("2".into(), 0.7),
                ]),
                confidence: Some(0.7),
            },
        })
    }
}

fn shared_fixtures() -> Vec<DecisionRequest> {
    vec![
        DecisionRequest {
            state: json!({ "fixture": "noul" }),
            instructions: "Noul fixture".into(),
            question: DecisionQuestion::Noul { criteria: None },
        },
        DecisionRequest {
            state: json!({ "fixture": "choice" }),
            instructions: "Choice fixture".into(),
            question: DecisionQuestion::Choice {
                options: BTreeMap::from([("no".into(), "No".into()), ("yes".into(), "Yes".into())]),
            },
        },
        DecisionRequest {
            state: json!({ "fixture": "score" }),
            instructions: "Score fixture".into(),
            question: DecisionQuestion::Score {
                levels: vec!["low".into(), "medium".into(), "high".into()],
            },
        },
    ]
}

#[test]
fn parity_decision_model_opencode_run_shared_fixtures() {
    let bundle = include_str!("../../../../../../plugins/provider-opencode/plugin.js");
    let setup = r#"
        host.exec = function() { return JSON.stringify({code: 0, stdout: 'fixture/model'}); };
        host.exec.async = function(opts, then) {
            var request = JSON.parse(opts.stdin);
            var config = JSON.parse(opts.env.OPENCODE_CONFIG_CONTENT);
            if (config.agent['domios-decision'].permission['*'] !== 'deny') throw new Error('unsafe fixture');
            var answer = request.question.type === 'noul' ? {type:'noul',p:0.8}
                : request.question.type === 'choice' ? {type:'choice',choice:'yes',probabilities:{no:0.2,yes:0.8},confidence:0.8}
                : {type:'score',score:2,probabilities:{'0':0.1,'1':0.2,'2':0.7},confidence:0.7};
            return then({code:0,stdout:JSON.stringify({type:'text',part:{text:JSON.stringify(answer)}})+'\n'+JSON.stringify({type:'step_finish',part:{reason:'stop'}})});
        };
        host.fs.readFile = function() { return null; };
    "#;
    let vm = Arc::new(PluginVm::from_source("run-fixture", format!("{setup}\n{bundle}")).unwrap());
    let javascript = JsDecisionModel::new(vm, "run-fixture", TokenMode::None);
    assert!(javascript.select_model("fixture/model"));
    assert!(!javascript.supports_question_batch());
    assert!(!javascript.supports_calibrated_choice_confidence());
    let native = NativeDecisionModel(TokenMode::None);
    for request in shared_fixtures() {
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        assert_eq!(
            javascript.decide(&request, deadline),
            native.decide(&request, deadline)
        );
    }
}

#[test]
fn parity_decision_model_typed_failures() {
    for (code, expected) in [
        (
            "unavailable",
            DecisionError::Unavailable("plugin_unavailable".into()),
        ),
        ("timeout", DecisionError::Deadline),
        (
            "transport",
            DecisionError::Transport("plugin_decide_failed".into()),
        ),
        (
            "invalid_response",
            DecisionError::Parse("invalid_answer".into()),
        ),
    ] {
        let source =
            format!("module.exports.default = {{ decide() {{ return {{error:'{code}'}}; }} }};");
        let model = JsDecisionModel::new(
            Arc::new(PluginVm::from_source("error-fixture", source).unwrap()),
            "error-fixture",
            TokenMode::None,
        );
        let request = shared_fixtures().remove(0);
        assert_eq!(
            model.decide(&request, Instant::now() + std::time::Duration::from_secs(2)),
            Err(expected)
        );
    }
}

#[test]
fn parity_decision_model_matches_shared_fixtures() {
    let source = r#"module.exports.default = {
        supportsCalibratedChoiceConfidence() { return true; },
        modelId() { return "fixture-model"; },
        selectModel(id) { return id === "fixture-model"; },
        metadata() { return {}; },
        pricing() { return { display_name: "local fixture", input_usd_per_million: 0, output_usd_per_million: 0 }; },
        primitives() { return { choice: true, noul: true, score: true }; },
        models() { return [{ id: "fixture-model", display_name: "Fixture model" }]; },
        decide(request) {
            if (request.question.type === "noul") return { type: "noul", p: 0.8 };
            if (request.question.type === "choice") return {
                type: "choice", choice: "yes", probabilities: { no: 0.2, yes: 0.8 }, confidence: 0.8
            };
            return {
                type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7
            };
        }
    };"#;
    let vm = Arc::new(PluginVm::from_source("js-fixture", source.into()).expect("vm"));
    let forms = [
        (json!(true), TokenMode::Optional),
        (json!({ "tokenMode": "required" }), TokenMode::Required),
        (json!({ "tokenMode": "none" }), TokenMode::None),
    ];
    for (form, expected) in forms {
        let caps: crate::js_runtime::manifest::Capabilities =
            serde_json::from_value(json!({ "decisionModel": form })).expect("manifest cap");
        let native = NativeDecisionModel(expected);
        let javascript =
            JsDecisionModel::new(vm.clone(), "js-fixture", caps.decision_model.token_mode);
        assert_eq!(native.token_mode(), expected);
        assert_eq!(
            native.supports_calibrated_choice_confidence(),
            javascript.supports_calibrated_choice_confidence()
        );
        assert_eq!(native.token_mode(), javascript.token_mode());
        assert_eq!(native.primitives(), javascript.primitives());
        assert_eq!(native.models(), javascript.models());
        assert_eq!(native.model_id(), javascript.model_id());
        assert_eq!(native.pricing(), javascript.pricing());
        assert_eq!(native.metadata(), javascript.metadata());
        assert!(native.select_model("fixture-model"));
        assert_eq!(
            native.select_model("fixture-model"),
            javascript.select_model("fixture-model")
        );
        assert_eq!(
            native.select_model("missing"),
            javascript.select_model("missing")
        );
        for request in shared_fixtures() {
            let deadline = Instant::now() + std::time::Duration::from_secs(2);
            assert_eq!(
                native.decide(&request, deadline),
                javascript.decide(&request, deadline),
            );
        }
    }
}

#[test]
fn js_decision_model_defaults_calibrated_choice_capability_to_false() {
    let model = JsDecisionModel::new(
        Arc::new(
            PluginVm::from_source(
                "decision-default-capability",
                "module.exports.default = {};".into(),
            )
            .unwrap(),
        ),
        "decision-default-capability",
        TokenMode::None,
    );
    assert!(!model.supports_calibrated_choice_confidence());
}

#[test]
fn parity_decision_model_multi_question_shared_state_and_sequential_local_adapter() {
    let decide = r#"decide(request) {
        if (JSON.stringify(request.state) !== JSON.stringify({ fixture: "shared" })) throw new Error("state mismatch");
        if (request.question.type === "noul") return { type: "noul", p: 0.8 };
        if (request.question.type === "choice") return { type: "choice", choice: "yes", probabilities: { no: 0.2, yes: 0.8 }, confidence: 0.8 };
        return { type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7 };
    }"#;
    let multi = r#"decideQuestions(request) {
        var answers = {};
        for (var name of Object.keys(request.questions)) {
            var spec = request.questions[name];
            answers[name] = this.decide({ state: request.state, instructions: spec.instructions, question: spec.question });
        }
        return { answers: answers, usage: null };
    }"#;
    let request = DecisionQuestionsRequest {
        state: json!({ "fixture": "shared" }),
        questions: shared_fixtures()
            .into_iter()
            .enumerate()
            .map(|(index, single)| {
                (
                    format!("question_{index}"),
                    NamedDecisionQuestion {
                        instructions: single.instructions,
                        question: single.question,
                    },
                )
            })
            .collect(),
    };
    for extension in ["", multi] {
        let source = format!("module.exports.default = {{ {decide}, {extension} }};");
        let vm = Arc::new(PluginVm::from_source("shared-fixture", source).unwrap());
        let js = JsDecisionModel::new(vm, "shared-fixture", TokenMode::None);
        let native = NativeDecisionModel(TokenMode::None);
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let response = js.decide_questions(&request, deadline).unwrap();
        assert_eq!(
            response,
            native.decide_questions(&request, deadline).unwrap()
        );
        assert_eq!(response.answers.len(), 3);
    }
}

#[test]
fn parity_decision_metadata_and_model_selection_apply_without_cross_adapter_leak() {
    use codeterm_types::decision::DecisionMetadata;
    struct Native(parking_lot::Mutex<String>);
    impl DecisionModel for Native {
        fn id(&self) -> &str {
            "fixture"
        }
        fn token_mode(&self) -> TokenMode {
            TokenMode::Optional
        }
        fn model_id(&self) -> Option<String> {
            Some(self.0.lock().clone())
        }
        fn primitives(&self) -> DecisionPrimitives {
            Default::default()
        }
        fn models(&self) -> Vec<DecisionModelInfo> {
            ["one", "two"]
                .into_iter()
                .map(|id| DecisionModelInfo {
                    id: id.into(),
                    display_name: id.into(),
                })
                .collect()
        }
        fn metadata(&self) -> DecisionMetadata {
            DecisionMetadata {
                help_url: Some("https://fixture.test/help".into()),
                server_address: Some("http://localhost:1234".into()),
                ..Default::default()
            }
        }
        fn select_model(&self, id: &str) -> bool {
            if !self.models().iter().any(|model| model.id == id) {
                return false;
            }
            *self.0.lock() = id.into();
            true
        }
        fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
            Err(DecisionError::Unsupported)
        }
    }
    let source = r#"var selected = "one";
        module.exports.default = {
            modelId() { return selected; },
            models() { return [{ id: "one", display_name: "one" }, { id: "two", display_name: "two" }]; },
            selectModel(id) { if (id !== "one" && id !== "two") return false; selected = id; return true; },
            metadata() { return { help_url: "https://fixture.test/help", server_address: "http://localhost:1234" }; }
        };"#;
    let javascript = JsDecisionModel::new(
        Arc::new(PluginVm::from_source("fixture", source.into()).unwrap()),
        "fixture",
        TokenMode::Optional,
    );
    let other = JsDecisionModel::new(
        Arc::new(PluginVm::from_source("other", source.into()).unwrap()),
        "other",
        TokenMode::Optional,
    );
    let native = Native(parking_lot::Mutex::new("one".into()));
    assert_eq!(native.metadata(), javascript.metadata());
    assert_eq!(native.select_model("two"), javascript.select_model("two"));
    assert_eq!(native.model_id(), javascript.model_id());
    assert_eq!(other.model_id().as_deref(), Some("one"));
    assert_eq!(
        native.select_model("missing"),
        javascript.select_model("missing")
    );
    assert_eq!(javascript.model_id().as_deref(), Some("two"));
}

#[test]
fn parity_decision_model_explicit_batch_capability() {
    struct NativeBatch(bool);
    impl DecisionModel for NativeBatch {
        fn supports_question_batch(&self) -> bool {
            self.0
        }
        fn id(&self) -> &str {
            "native-batch"
        }
        fn token_mode(&self) -> TokenMode {
            TokenMode::None
        }
        fn primitives(&self) -> DecisionPrimitives {
            Default::default()
        }
        fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
            Err(DecisionError::Unsupported)
        }
    }
    for declared in [false, true] {
        let source = format!("module.exports.default = {{ supportsQuestionBatch() {{ return {declared}; }}, decideQuestions() {{ return {{ answers: {{}}, usage: null }}; }} }};");
        let vm = Arc::new(PluginVm::from_source("batch-fixture", source).unwrap());
        let js = JsDecisionModel::new(vm, "batch-fixture", TokenMode::None);
        assert_eq!(
            js.supports_question_batch(),
            NativeBatch(declared).supports_question_batch()
        );
    }
    for source in [
        "module.exports.default = { decideQuestions() { return null; } };",
        "module.exports.default = { supportsQuestionBatch() { return true; } };",
    ] {
        let implicit = Arc::new(PluginVm::from_source("implicit", source.into()).unwrap());
        assert!(
            !JsDecisionModel::new(implicit, "implicit", TokenMode::None).supports_question_batch()
        );
    }
    assert!(!NativeDecisionModel(TokenMode::None).supports_question_batch());
}
