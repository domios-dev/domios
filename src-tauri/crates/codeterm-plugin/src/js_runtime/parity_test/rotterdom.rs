use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionQuestionsRequest, DecisionRequest,
    DecisionUsage, NamedDecisionQuestion, TokenMode,
};
use serde::Deserialize;
use serde_json::json;

use super::decision_model::NativeDecisionModel;
use crate::js_runtime::{JsDecisionModel, PluginVm};

#[derive(Deserialize)]
struct Fixture {
    request: DecisionRequest,
    answer: DecisionAnswer,
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("fixtures/rotterdom/shared.json")).unwrap()
}

#[test]
fn rotterdom_open_session_persists_history_in_a_missing_private_state_root() {
    use crate::js_runtime::host_api::PluginSandbox;
    use crate::js_runtime::vm::call_json;

    let home = tempfile::tempdir().unwrap();
    let bundle = home.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let data = home.path().join(".codeterm-dev/rotterdom");
    let manifest = crate::js_runtime::PluginManifest::from_json_str(include_str!(
        "fixtures/rotterdom/plugin.json"
    ))
    .unwrap();
    let (reads, writes) = crate::js_runtime::loader::confined_roots(&manifest, &bundle, &data);
    let settings = json!({
        "endpoints": [{"id": "local", "baseUrl": "http://127.0.0.1:1234/v1", "models": ["fixture-model"]}],
        "profiles": [{"id": "coding", "endpoint": "local", "model": "fixture-model", "root": ""}],
        "defaultProfile": "coding"
    });
    let sandbox = Arc::new(
        PluginSandbox::restricted(reads, writes)
            .with_project_roots(Vec::new(), Vec::new(), bundle.join("grants.yaml"))
            .with_settings(settings.to_string()),
    );
    assert!(!data.exists());
    assert!(sandbox.can_write(data.to_str().unwrap()));
    let setup = format!(
        "host.fs.expandHome = function(path) {{ return path === '~/.codeterm/rotterdom' ? {} : null; }};",
        serde_json::to_string(&data).unwrap()
    );
    let vm = PluginVm::from_source_sandboxed(
        "rotterdom",
        format!("{setup}\n{}", include_str!("fixtures/rotterdom/plugin.js")),
        None,
        sandbox,
    )
    .unwrap();
    let result: serde_json::Value =
        call_json(&vm, "openSession", &[json!({"tabId": "history-fixture"})]).unwrap();
    assert_eq!(result, json!({"sessionId": "history-fixture"}));
    let history: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data.join("session-history-fixture.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(history["id"], "history-fixture");
    assert_eq!(history["profile"]["root"], "");
    assert_eq!(history["turns"], json!([]));
    assert!(!home.path().join(".codeterm").exists());
}

fn model(failure: &str) -> JsDecisionModel {
    let setup = r#"
        var failure = FAILURE;
        var fixtures = FIXTURES;
        function equalJson(left, right) {
            function ordered(_key, value) {
                if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
                var result = {};
                Object.keys(value).sort().forEach(function(key) { result[key] = value[key]; });
                return result;
            }
            return JSON.stringify(left, ordered) === JSON.stringify(right, ordered);
        }
        var clock = 100;
        host.unixNowMs = function() { return clock; };
        host.settingsJson = function() { return JSON.stringify({
            endpoints:[{id:'local',baseUrl:'http://localhost:1234/v1',models:[]}],
            profiles:[{id:'decision',endpoint:'local',model:'fixture-model',root:'',
                budgets:{tokens:4096,deadlineMs:10000}}],defaultProfile:'decision'
        }); };
        host.projectWriteAllowed = function() { throw new Error('decision requested filesystem grants'); };
        host.fetch.async = function(opts, then) {
            if (opts.headers.authorization) throw new Error('unexpected token');
            if (opts.method === 'GET') {
                if (opts.url !== 'http://localhost:1234/v1/models') throw new Error('wrong catalogue URL');
                return then({status:200,body:JSON.stringify({data:[{id:'fixture-model',name:'Fixture model'}]})});
            }
            if (opts.url !== 'http://localhost:1234/v1/chat/completions') throw new Error('wrong completion URL');
            if (!(opts.timeoutMs > 0 && opts.timeoutMs <= 2000)) throw new Error('deadline ignored');
            var body = JSON.parse(opts.body);
            if (body.model !== 'fixture-model' || body.tools || body.tool_choice || body.stream !== false) throw new Error('unsafe decision wire');
            if (body.response_format.type !== 'json_schema' || !body.response_format.json_schema.strict) throw new Error('unconstrained decision');
            if (failure === 'transport') return then({status:503,body:'private error'});
            if (failure === 'timeout') { clock += 3000; return then({error:'timed out'}); }
            if (failure === 'malformed') return then({status:200,body:'invalid JSON'});
            var input = JSON.parse(body.messages[1].content);
            if (input.questions.single && !equalJson(input.state, fixtures.find(function(f) { return f.request.question.type === input.questions.single.question.type; }).request.state)) throw new Error('state changed');
            if (!input.questions.single && input.state.fixture !== 'shared') throw new Error('batch state changed');
            var answers = {};
            Object.keys(input.questions).forEach(function(name) {
                var spec = input.questions[name];
                var fixture = fixtures.find(function(f) { return f.request.question.type === spec.question.type; });
                if (spec.instructions !== fixture.request.instructions || !equalJson(spec.question, fixture.request.question)) throw new Error('question changed');
                answers[name] = fixture.answer;
            });
            var answer = Object.keys(input.questions)[0] === 'single' ? answers.single : {answers:answers};
            return then({status:200,body:JSON.stringify({choices:[{message:{content:JSON.stringify(answer)},finish_reason:'stop'}],usage:{prompt_tokens:123,completion_tokens:45}})});
        };
    "#
    .replace("FAILURE", &serde_json::to_string(failure).unwrap())
    .replace("FIXTURES", include_str!("fixtures/rotterdom/shared.json"));
    let bundle = include_str!("fixtures/rotterdom/plugin.js");
    let vm = Arc::new(PluginVm::from_source("rotterdom", format!("{setup}\n{bundle}")).unwrap());
    let manifest: crate::js_runtime::manifest::PluginManifest =
        serde_json::from_str(include_str!("fixtures/rotterdom/plugin.json")).unwrap();
    assert!(manifest.capabilities.decision_model.enabled);
    assert_eq!(
        manifest.capabilities.decision_model.token_mode,
        TokenMode::None
    );
    JsDecisionModel::new(
        vm,
        "rotterdom",
        manifest.capabilities.decision_model.token_mode,
    )
}

#[test]
fn parity_rotterdom_decision_adapter_shared_fixtures() {
    let javascript = model("");
    let native = NativeDecisionModel(TokenMode::None);
    assert_eq!(javascript.token_mode(), native.token_mode());
    assert_eq!(javascript.primitives(), native.primitives());
    assert_eq!(javascript.models(), native.models());
    assert_eq!(javascript.model_id(), native.model_id());
    assert!(javascript.select_model("fixture-model"));
    assert!(!javascript.select_model("missing"));
    assert!(!javascript.supports_calibrated_choice_confidence());
    assert!(javascript.supports_question_batch());
    assert_eq!(
        javascript.metadata().display_name.as_deref(),
        Some("Rotterdom")
    );
    for fixture in fixtures() {
        let deadline = Instant::now() + Duration::from_secs(2);
        assert_eq!(
            native.decide(&fixture.request, deadline).unwrap(),
            fixture.answer
        );
        assert_eq!(
            javascript.decide(&fixture.request, deadline),
            native.decide(&fixture.request, deadline)
        );
    }
    let request = DecisionQuestionsRequest {
        state: json!({ "fixture": "shared" }),
        questions: fixtures()
            .into_iter()
            .map(|f| {
                (
                    f.request.state["fixture"].as_str().unwrap().into(),
                    NamedDecisionQuestion {
                        instructions: f.request.instructions,
                        question: f.request.question,
                    },
                )
            })
            .collect(),
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut expected = native.decide_questions(&request, deadline).unwrap();
    expected.usage = Some(DecisionUsage {
        input_tokens: 123,
        output_tokens: 45,
    });
    assert_eq!(
        javascript.decide_questions(&request, deadline).unwrap(),
        expected
    );
}

#[test]
fn parity_rotterdom_decision_adapter_typed_failures() {
    let request = fixtures().remove(0).request;
    for (failure, expected) in [
        (
            "transport",
            DecisionError::Transport("plugin_decide_failed".into()),
        ),
        ("timeout", DecisionError::Deadline),
        ("malformed", DecisionError::Parse("invalid_answer".into())),
    ] {
        assert_eq!(
            model(failure).decide(&request, Instant::now() + Duration::from_secs(2)),
            Err(expected)
        );
    }
    assert_eq!(
        model("").decide(&request, Instant::now()),
        Err(DecisionError::Deadline)
    );
}
