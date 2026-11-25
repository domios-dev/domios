use super::*;
use codeterm_service_core::mesh::crown::{
    generate_peer_keypair, sign_request_with_agent, sign_request_with_origin, AuthenticatedAgent,
    AuthenticatedOrigin,
};
use codeterm_service_core::mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::provider_cli_actions::CliOperation;
use codeterm_service_core::service::test_fixture::ServiceFixture;

fn bearer_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers
}

fn paired_peer(id: &str, public_key: String, access_level: AccessLevel) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[tokio::test]
async fn relayed_provider_cli_update_accepts_owner_origin_and_refuses_mesh_or_agent_origins() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let (sk, pk) = generate_peer_keypair();
    state
        .mesh
        .lock()
        .insert_account_peer_for_test(paired_peer("peer-full", pk, AccessLevel::Full));

    for caller_tab_id in [Some(tab_id.clone()), None] {
        let owner_token =
            sign_request_with_origin(&sk, "peer-full", AuthenticatedOrigin::Owner).unwrap();
        let err = match run(
            State(state.clone()),
            bearer_headers(&owner_token),
            Path("missing-provider".into()),
            Json(RunBody {
                operation: CliOperation::Update,
                caller_tab_id,
            }),
        )
        .await
        {
            Ok(_) => panic!("unknown provider should fail after caller authorization"),
            Err(err) => err,
        };
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("Unknown provider"), "{}", err.1);
    }

    let mesh_token = sign_request_with_origin(&sk, "peer-full", AuthenticatedOrigin::Mesh).unwrap();
    let mesh_err = match run(
        State(state.clone()),
        bearer_headers(&mesh_token),
        Path("missing-provider".into()),
        Json(RunBody {
            operation: CliOperation::Update,
            caller_tab_id: Some(tab_id.clone()),
        }),
    )
    .await
    {
        Ok(_) => panic!("mesh origin must be rejected"),
        Err(err) => err,
    };
    assert_eq!(mesh_err.0, StatusCode::FORBIDDEN);

    let agent_token = sign_request_with_agent(
        &sk,
        "peer-full",
        AuthenticatedOrigin::Agent,
        Some(&AuthenticatedAgent {
            instance_id: "peer-full".into(),
            tab_id: "remote-agent".into(),
            can_spawn: true,
        }),
    )
    .unwrap();
    let agent_err = match run(
        State(state),
        bearer_headers(&agent_token),
        Path("missing-provider".into()),
        Json(RunBody {
            operation: CliOperation::Update,
            caller_tab_id: Some(tab_id),
        }),
    )
    .await
    {
        Ok(_) => panic!("remote agent origin must be rejected"),
        Err(err) => err,
    };
    assert_eq!(agent_err.0, StatusCode::FORBIDDEN);
}
