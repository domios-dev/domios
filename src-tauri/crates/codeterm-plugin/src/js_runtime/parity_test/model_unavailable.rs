use super::*;

#[tokio::test]
async fn model_unavailable_round_trips_the_runtime_refusal() {
    use crate::agent_runtime::{
        RpcReply, RuntimeContext, RuntimeFailureCode, RuntimeOperation, RuntimeSettings,
        RuntimeStep, TurnInput,
    };
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/runtime-model-unavailable.json"
    ))
    .unwrap();
    let model = fixture["model"].as_str().unwrap().to_owned();
    let settings = RuntimeSettings {
        model: Some(model),
        ..Default::default()
    };
    let ctx = RuntimeContext {
        thread_id: Some("own".into()),
        settings: settings.clone(),
        ..Default::default()
    };
    let op = RuntimeOperation::SubmitTurn {
        thread_id: "own".into(),
        active_turn_id: None,
        send_id: "claim".into(),
        input: TurnInput {
            text: "opaque".into(),
            attachments: Vec::new(),
        },
        settings,
    };
    let reply = RpcReply::Error {
        error: serde_json::from_value(fixture["rejection"].clone()).unwrap(),
    };
    let provider = load_js(fixture["provider"].as_str().unwrap());
    let plan = provider.runtime_plan(&ctx, &op).await.unwrap();
    let step = provider
        .runtime_response(&ctx, &op, &plan.state, &reply)
        .await
        .unwrap();
    let RuntimeStep::Refused {
        reason,
        before_acceptance: true,
    } = step
    else {
        panic!("Expected a pre-acceptance refusal")
    };
    assert_eq!(reason.code, RuntimeFailureCode::ModelUnavailable);
}
