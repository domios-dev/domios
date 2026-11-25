use super::*;
use codeterm_account::account_mesh::wire::{
    Eligibility, MeshEndpointObservation, PublicationState,
};

fn row(
    id: &str,
    name: Option<&str>,
    admitted_at: u64,
    active: bool,
    presence: RegistryPresence,
    membership_expires_at: Option<u64>,
    endpoint_expires_at: Option<u64>,
) -> MeshManagementRow {
    MeshManagementRow {
        installation_id: id.into(),
        key_id: "a".repeat(64),
        key_generation: 1,
        slot_revision: 1,
        active,
        presence,
        admitted_at,
        authenticated_at: admitted_at,
        display_name: name.map(str::to_owned),
        metadata_revision: 1,
        publication: PublicationState::Complete,
        membership_expires_at,
        eligibility: if active {
            Eligibility::Eligible
        } else {
            Eligibility::Revoked
        },
        endpoint: endpoint_expires_at.map(|expires_at| MeshEndpointObservation {
            transport: None,
            tailnet_ip: "192.0.2.10".into(),
            api_port: 7685,
            expires_at,
        }),
    }
}

#[test]
fn resolve_remove_target_refuses_self_by_id_instance_and_label() {
    let rows = vec![
        row(
            "self-id",
            Some("BLACKSTAR [dev-1]"),
            10,
            true,
            RegistryPresence::Present,
            Some(200),
            Some(200),
        ),
        row(
            "peer-id",
            Some("BLACKSTAR [dev-2]"),
            20,
            true,
            RegistryPresence::Present,
            Some(200),
            Some(200),
        ),
        row(
            "unique-id",
            Some("NULL.Home [dev]"),
            30,
            true,
            RegistryPresence::Present,
            Some(50),
            None,
        ),
    ];
    assert_eq!(
        resolve_remove_target("self-id", "self-id", Some("local-inst"), &rows, &[]),
        Err(RemoveTargetError::CannotRemoveSelf)
    );
    assert_eq!(
        resolve_remove_target("local-inst", "self-id", Some("local-inst"), &rows, &[]),
        Err(RemoveTargetError::CannotRemoveSelf)
    );
    assert_eq!(
        resolve_remove_target("BLACKSTAR #1", "self-id", Some("local-inst"), &rows, &[]),
        Err(RemoveTargetError::CannotRemoveSelf)
    );
    assert_eq!(
        resolve_remove_target(
            "BLACKSTAR [dev-1]",
            "self-id",
            Some("local-inst"),
            &rows,
            &[]
        ),
        Err(RemoveTargetError::CannotRemoveSelf)
    );
}

#[test]
fn resolve_remove_target_matches_id_label_clean_name_and_reports_ambiguity() {
    let rows = vec![
        row(
            "self-id",
            Some("BLACKSTAR [dev-1]"),
            10,
            true,
            RegistryPresence::Present,
            Some(200),
            Some(200),
        ),
        row(
            "peer-2",
            Some("BLACKSTAR [dev-2]"),
            20,
            true,
            RegistryPresence::Present,
            Some(200),
            Some(200),
        ),
        row(
            "peer-3",
            Some("BLACKSTAR [dev-3]"),
            30,
            true,
            RegistryPresence::SignedOut,
            Some(50),
            None,
        ),
        row(
            "stale-null",
            Some("NULL.Home [dev-old]"),
            40,
            true,
            RegistryPresence::Present,
            Some(50),
            None,
        ),
        row(
            "gone-id",
            Some("Old"),
            5,
            false,
            RegistryPresence::Present,
            Some(10),
            None,
        ),
    ];
    let pairs = vec![("pair-inst-2".to_owned(), "peer-2".to_owned())];

    assert_eq!(
        resolve_remove_target("BLACKSTAR #3", "self-id", None, &rows, &pairs)
            .unwrap()
            .row
            .installation_id,
        "peer-3"
    );
    assert_eq!(
        resolve_remove_target("NULL.Home", "self-id", None, &rows, &pairs)
            .unwrap()
            .row
            .installation_id,
        "stale-null"
    );
    assert_eq!(
        resolve_remove_target("pair-inst-2", "self-id", None, &rows, &pairs)
            .unwrap()
            .row
            .installation_id,
        "peer-2"
    );
    assert_eq!(
        resolve_remove_target("gone-id", "self-id", None, &rows, &pairs),
        Err(RemoveTargetError::AlreadyRemoved("gone-id".into()))
    );
    match resolve_remove_target("BLACKSTAR", "self-id", None, &rows, &pairs) {
        Err(RemoveTargetError::Ambiguous { matches, .. }) => {
            assert_eq!(matches.len(), 2);
            assert_eq!(matches[0].label, "BLACKSTAR #2");
            assert_eq!(matches[1].label, "BLACKSTAR #3");
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn is_row_online_distinguishes_live_from_stale_or_signed_out() {
    let live = row(
        "live",
        Some("A"),
        1,
        true,
        RegistryPresence::Present,
        Some(200),
        Some(200),
    );
    let expired = row(
        "exp",
        Some("B"),
        1,
        true,
        RegistryPresence::Present,
        Some(90),
        Some(200),
    );
    let stale_ep = row(
        "ep",
        Some("C"),
        1,
        true,
        RegistryPresence::Present,
        Some(200),
        Some(90),
    );
    let signed_out = row(
        "out",
        Some("D"),
        1,
        true,
        RegistryPresence::SignedOut,
        Some(200),
        Some(200),
    );

    assert!(is_row_online(&live, false, 100));
    assert!(!is_row_online(&expired, false, 100));
    assert!(is_row_online(&expired, true, 100));
    assert!(!is_row_online(&stale_ep, false, 100));
    assert!(!is_row_online(&signed_out, false, 100));
}
