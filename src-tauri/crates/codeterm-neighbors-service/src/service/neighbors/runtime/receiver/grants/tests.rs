use super::*;

fn preview(previous: Option<Permission>) -> Preview {
    Preview::new(
        &ReceiverStatus {
            receiver_installation_id: Some("air".into()),
            receiver_encryption_key_id: None,
            receiver_machine_label: Some("air".into()),
            receiver_revision: 3,
            is_me: false,
            receiving_enabled: true,
        },
        json!(Permission::default()),
        previous.map(|grant| json!(grant)),
        "owner",
        "BLACKSTAR",
        "friend",
    )
    .unwrap()
}

fn full() -> Permission {
    Permission {
        allowed: true,
        machine_access: MachineAccess::Full,
        scope: "Share only the joint project".into(),
        memory_write_areas: vec!["joint".into()],
        ..Permission::default()
    }
}

fn decision(preview: &Preview, choice: Choice) -> Decision {
    Decision {
        choice,
        expected_digest: preview.digest.clone(),
        scope: None,
    }
}

#[test]
fn receiver_grants_compare_keep_and_adopt_without_silent_transfer() {
    let original = full();
    let preview = preview(Some(original.clone()));
    assert!(preview.different);
    assert!(preview
        .decide(None, true)
        .unwrap_err()
        .contains("receiver_grant_confirmation_required"));
    assert!(preview
        .decide(Some(&decision(&preview, Choice::Keep)), true)
        .unwrap()
        .is_none());
    assert_eq!(
        preview
            .decide(Some(&decision(&preview, Choice::Adopt)), true)
            .unwrap(),
        Some(original)
    );
    assert!(preview
        .decide(Some(&decision(&preview, Choice::Adopt)), false)
        .is_err());
}

#[test]
fn receiver_grants_bind_confirmation_to_both_snapshots_and_receiver() {
    let before = preview(Some(full()));
    let mut changed = full();
    changed.allow_files = false;
    let after = preview(Some(changed));
    assert!(after
        .decide(Some(&decision(&before, Choice::Adopt)), true)
        .unwrap_err()
        .contains("receiver_grant_stale"));
    let mut local = Permission::default();
    local.allow_memory = false;
    let status = ReceiverStatus {
        receiver_installation_id: Some("air".into()),
        receiver_machine_label: Some("air".into()),
        receiver_encryption_key_id: None,
        receiver_revision: 4,
        is_me: false,
        receiving_enabled: true,
    };
    let changed = Preview::new(
        &status,
        json!(local),
        Some(json!(full())),
        "owner",
        "BLACKSTAR",
        "friend",
    )
    .unwrap();
    assert_ne!(changed.digest, before.digest);
    assert!(changed
        .decide(Some(&decision(&before, Choice::Keep)), true)
        .is_err());
    for (uid, target, friend) in [
        ("other-owner", "BLACKSTAR", "friend"),
        ("owner", "third-machine", "friend"),
        ("owner", "BLACKSTAR", "other-friend"),
    ] {
        let mut status = status.clone();
        status.receiver_revision = 3;
        let changed = Preview::new(
            &status,
            json!(Permission::default()),
            Some(json!(full())),
            uid,
            target,
            friend,
        )
        .unwrap();
        assert!(changed
            .decide(Some(&decision(&before, Choice::Adopt)), true)
            .is_err());
    }
}

#[test]
fn receiver_grants_equal_or_unavailable_never_enable_adoption() {
    let equal = preview(Some(Permission::default()));
    assert!(!equal.different);
    assert!(equal.decide(None, false).unwrap().is_none());
    let missing = preview(None);
    assert!(missing.unavailable);
    assert!(missing
        .decide(Some(&decision(&missing, Choice::Adopt)), true)
        .is_err());
}

#[test]
fn receiver_grants_require_reconciled_scope_without_discarding_sharing_limits() {
    let mut previous = full();
    previous.scope = "Read-only commands. Share only the joint project.".into();
    let preview = preview(Some(previous));
    let mut adopt = decision(&preview, Choice::Adopt);
    assert!(preview
        .decide(Some(&adopt), true)
        .unwrap_err()
        .contains("Reconcile"));
    adopt.scope = Some("Full machine access. Share only the joint project.".into());
    let adopted = preview.decide(Some(&adopt), true).unwrap().unwrap();
    assert_eq!(adopted.machine_access, MachineAccess::Full);
    assert_eq!(
        adopted.scope,
        "Full machine access. Share only the joint project."
    );
    assert_eq!(adopted.memory_write_areas, ["joint"]);
}

#[test]
fn receiver_grants_never_read_a_foreign_account_or_another_installation() {
    use codeterm_mesh::account_mesh::ledger::{LinkAdmission, LinkRecord};
    let link = LinkRecord {
        link_nonce: None,
        remote_membership: None,
        admission: LinkAdmission::Account {
            account_id: codeterm_account::account_mesh::account_id("owner"),
            installation_id: "air".into(),
            public_key: "key".into(),
        },
    };
    assert!(owns_receiver_link(
        &codeterm_account::account_mesh::account_id("owner"),
        Some("air"),
        &link
    ));
    assert!(!owns_receiver_link(
        &codeterm_account::account_mesh::account_id("friend"),
        Some("air"),
        &link
    ));
    assert!(!owns_receiver_link(
        &codeterm_account::account_mesh::account_id("owner"),
        Some("BLACKSTAR"),
        &link
    ));
    assert!(!owns_receiver_link(
        &codeterm_account::account_mesh::account_id("owner"),
        None,
        &link
    ));
}

#[test]
fn receiver_grants_select_a_signed_client_using_the_opaque_mesh_account() {
    use codeterm_mesh::{
        account_mesh::{AccountMeshState, AccountPresence, SignedMembership, VerifiedMembership},
        crown::generate_peer_keypair,
        types::{AccessLevel, MeshIdentity, PairedPeer, PeerAddress, RELAY_PROTOCOL_VERSION},
        MeshManager,
    };
    let (private, public) = generate_peer_keypair();
    let mut mesh = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "target-instance".into(),
            private_key: Some(private),
            public_key: Some(public.clone()),
        },
        "Target".into(),
        7697,
    );
    let account = codeterm_account::account_mesh::account_id("owner");
    assert_ne!(account, "owner");
    let expires_at = mesh.now() + 3600;
    mesh.apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: account.clone(),
        },
        membership: Some(SignedMembership {
            token: "opaque-membership".into(),
            verified: VerifiedMembership {
                account_id: account,
                installation_id: "target-installation".into(),
                key_id: "fixture-key".into(),
                key_generation: 1,
                slot_revision: 1,
                issued_at: 0,
                expires_at,
            },
        }),
        ..Default::default()
    });
    mesh.insert_account_peer_for_test(PairedPeer {
        instance_id: "source-instance".into(),
        instance_name: "Source".into(),
        addresses: vec![PeerAddress {
            ip: "100.84.159.37".into(),
            is_tailscale: true,
        }],
        api_port: 7707,
        public_key: public,
        paired_at: "fixture".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    let mut status = ReceiverStatus {
        receiver_installation_id: Some("source-instance".into()),
        receiver_encryption_key_id: None,
        receiver_machine_label: Some("Source".into()),
        receiver_revision: 4,
        is_me: false,
        receiving_enabled: true,
    };
    assert!(mesh.traffic_open("source-instance"));
    assert!(receiver_client(&mesh, "owner", &status).is_ok());
    assert!(receiver_client(&mesh, "other-owner", &status).is_err());
    status.receiver_installation_id = Some("another-installation".into());
    assert!(receiver_client(&mesh, "owner", &status).is_err());
    status.receiver_installation_id = Some("source-instance".into());
    mesh.account_installations[0].signed_out = true;
    assert!(receiver_client(&mesh, "owner", &status).is_err());
    mesh.account_installations[0].signed_out = false;
    let mut revoked = mesh.account_state().clone();
    revoked
        .revoked_installations
        .insert("source-instance".into());
    mesh.apply_account_state(revoked);
    assert!(receiver_client(&mesh, "owner", &status).is_err());
}
