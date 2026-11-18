use super::*;

#[test]
fn concierge_admission_refuses_owner_and_agents_but_accepts_its_routed_friend() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};
    use crate::service::message_gateway::DeliverySeam;
    use crate::service::messenger::{HumanInput, MessageEnvelope, NeighborMessage};
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "friend".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.bind_recorded_agent_recipient(&id, "own-session", "own-nonce");
    let pty_id = format!("recorded-{id}");
    let service = &fixture.service;
    let error = "concierge_read_only: Concierge for your friend — message your friend in Neighbors";
    let refusal = crate::service::delivery_error::DeliveryError::from_opaque(&id, error);
    assert_eq!(refusal.to_json()["error"]["code"], "concierge_read_only");
    assert_eq!(refusal.to_json()["error"]["bytes_written"], false);
    for source in [None, Some(id.as_str()), Some("other-agent")] {
        assert_eq!(
            service
                .require_managed_send_allowed(&id, source)
                .unwrap_err(),
            error
        );
    }
    for bytes in [b"opaque".as_slice(), b"\x1b".as_slice()] {
        assert_eq!(
            service
                .require_managed_terminal_input_allowed(&pty_id, bytes)
                .unwrap_err(),
            error
        );
    }
    assert_eq!(
        service
            .start_human_chat_send(&id, HumanInput::composer("opaque"), None, None)
            .unwrap_err(),
        error
    );
    let owner = MessageEnvelope::tab("owner", "other-agent");
    assert_eq!(
        service
            .deliver_enveloped(&id, owner, "opaque owner payload")
            .unwrap_err(),
        error
    );
    let mut friend = MessageEnvelope::neighbor(
        "friend".into(),
        "friendship".into(),
        "Ada".into(),
        NeighborMessage {
            message_id: "opaque-id".into(),
            kind: codeterm_neighbors::wire::Kind::Question,
            in_reply_to: None,
        },
        "opaque-delivery".into(),
    );
    assert!(writes.texts.lock().is_empty());
    service
        .deliver_enveloped(&id, friend.clone(), "opaque friend payload")
        .unwrap();
    assert_eq!(writes.texts.lock().as_slice(), ["opaque friend payload"]);
    friend.from_neighbor = Some("other-friendship".into());
    assert_eq!(
        service
            .require_enveloped_input_allowed(&id, &friend)
            .unwrap_err(),
        error
    );
    assert!(service
        .require_enveloped_input_allowed(&id, &MessageEnvelope::codeterm_reminder("routing"))
        .is_ok());
    let admitted = service
        .attach_system_send(&id, "opaque routed context")
        .unwrap();
    assert!(service
        .require_admitted_input_allowed(&id, Some(&admitted.send_id))
        .is_ok());
    service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .neighbor_authority
        .as_mut()
        .unwrap()
        .root_tab_id = Some("root".into());
    assert!(service.require_tab_input_allowed(&id, None).is_ok());
}

#[test]
fn curator_input_refuses_users_and_foreign_agents_but_accepts_its_orchestrator() {
    assert_eq!(
        authorize(true, None, Some("owner")),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(
        authorize(true, Some("foreign"), Some("owner")),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(authorize(true, Some("owner"), Some("owner")), Ok(()));
    assert_eq!(
        authorize(true, Some("owner"), None),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(authorize(false, None, None), Ok(()));
}

#[test]
fn curator_user_entry_points_refuse_before_pty_or_ledger_work() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().role_profile = Some("curator".into());
    mux.link_pty(&id, "round-pty".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let error = "curator_read_only: Curator round, read-only";
    assert_eq!(
        service.require_managed_send_allowed(&id, None).unwrap_err(),
        error
    );
    assert_eq!(
        service
            .require_managed_terminal_input_allowed("round-pty", b"a")
            .unwrap_err(),
        error
    );
    assert_eq!(
        service
            .require_managed_terminal_input_allowed("round-pty", b"\x1b")
            .unwrap_err(),
        error
    );
    assert!(service.require_admitted_input_allowed(&id, None).is_err());
    assert!(service
        .require_admitted_input_allowed(&id, Some("unknown"))
        .is_err());
    let admitted = service
        .attach_system_send(&id, "opaque host envelope")
        .unwrap();
    assert!(service
        .require_admitted_input_allowed(&id, Some(&admitted.send_id))
        .is_ok());
    assert_eq!(
        service
            .start_human_chat_send(
                &id,
                crate::service::messenger::HumanInput::composer("opaque"),
                None,
                None,
            )
            .unwrap_err(),
        error
    );
}

#[test]
fn curator_input_resolves_the_current_plan_owner_instead_of_any_sender() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.get_active_workspace().unwrap().id.clone();
    let curator = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&curator).unwrap().role_profile = Some("curator".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    service
        .set_ext(super::super::curator_runtime::CuratorTabs::default())
        .ok()
        .unwrap();
    let mut plan = Plan::new(
        "Fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    service.plan_store.save(&mut plan, 1).unwrap();
    service
        .ext::<super::super::curator_runtime::CuratorTabs>()
        .0
        .write()
        .insert(curator.clone(), plan.id.clone());
    assert!(service
        .require_managed_send_allowed(&curator, Some(&owner))
        .is_ok());
    assert!(service
        .require_managed_send_allowed(&curator, None)
        .unwrap_err()
        .starts_with("curator_read_only:"));
}
