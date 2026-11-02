use super::*;

#[test]
fn actor_authority_requires_owner_or_credential_bound_local_tab() {
    let tab = TabCaller {
        tab_id: "own-tab".into(),
        binding_nonce: "nonce".into(),
    };
    for (origin, identity, local_actor) in [
        (AuthenticatedOrigin::Owner, CallerIdentity::Local, true),
        (AuthenticatedOrigin::Agent, CallerIdentity::Tab(tab), true),
        (AuthenticatedOrigin::Agent, CallerIdentity::Relayed, false),
        (AuthenticatedOrigin::Mesh, CallerIdentity::Local, false),
        (AuthenticatedOrigin::Service, CallerIdentity::Relayed, false),
    ] {
        for header in [None, Some("own-tab"), Some("foreign-tab")] {
            let mut headers = HeaderMap::new();
            headers.insert("x-codeterm-authenticated-origin", "Owner".parse().unwrap());
            if let Some(header) = header {
                headers.insert("x-codeterm-tab-id", header.parse().unwrap());
            }
            let expected = local_actor
                && !(origin == AuthenticatedOrigin::Agent && header == Some("foreign-tab"));
            let result = ActorHeaders::authenticated(&identity, origin, headers);
            assert_eq!(result.is_ok(), expected, "{origin:?} header={header:?}");
            if let Ok(headers) = result {
                assert_eq!(
                    crate::tab_routes::caller_tab_header(&headers).as_deref(),
                    if origin == AuthenticatedOrigin::Agent {
                        Some("own-tab")
                    } else {
                        header
                    }
                );
            }
        }
    }
}

#[test]
fn actor_scope_matrix_never_grants_a_remote_agent_local_or_owner_authority() {
    let local = TabCaller {
        tab_id: "local".into(),
        binding_nonce: "nonce".into(),
    };
    let remote = codeterm_service_core::mesh::crown::AuthenticatedAgent {
        instance_id: "remote-machine".into(),
        tab_id: "remote-tab".into(),
        can_spawn: true,
    };
    for (actor, local_scope, agent_scope) in [
        (RequestActor::Owner, true, true),
        (RequestActor::LocalAgent(local), true, true),
        (RequestActor::RemoteAgent(remote), false, true),
        (RequestActor::Peer, false, true),
        (RequestActor::Service, false, false),
    ] {
        for selected in [None, Some("local"), Some("foreign")] {
            let mut headers = HeaderMap::new();
            if let Some(selected) = selected {
                headers.insert("x-codeterm-tab-id", selected.parse().unwrap());
            }
            let bound = !matches!(actor, RequestActor::LocalAgent(_))
                || selected.is_none()
                || selected == Some("local");
            assert_eq!(
                ActorHeaders::from_actor(actor.clone(), headers.clone()).is_ok(),
                local_scope && bound,
                "{actor:?} {selected:?}"
            );
            let result = AgentHeaders::from_actor(actor.clone(), headers);
            assert_eq!(
                result.is_ok(),
                agent_scope && bound,
                "{actor:?} {selected:?}"
            );
            if let (RequestActor::RemoteAgent(_), Ok(headers)) = (&actor, result) {
                assert!(headers.local_tab_id().is_none());
                assert!(headers.get("x-codeterm-tab-id").is_none());
            }
        }
    }
}
