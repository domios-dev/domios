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
