use super::*;
use codeterm_account::account_mesh::wire::{
    Eligibility, MeshEndpointObservation, PublicationState,
};

const NOW: u64 = 1_800_000_000;

fn row(
    id: &str,
    name: Option<&str>,
    authenticated_at: u64,
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
        admitted_at: authenticated_at,
        authenticated_at,
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
            tailnet_ip: "100.64.0.10".into(),
            api_port: 7685,
            expires_at,
        }),
    }
}

#[test]
fn default_selection_picks_expired_signed_out_and_long_unreachable_and_spares_self_online_recent() {
    let rows = vec![
        // Self: must never be selected even if expired.
        row(
            "self-id",
            Some("My-PC"),
            NOW - 30 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 10),
            None,
        ),
        // Already revoked: ignored.
        row(
            "revoked-id",
            Some("Old-Revoked"),
            NOW - 30 * SECONDS_PER_DAY,
            false,
            RegistryPresence::Present,
            Some(NOW - 100),
            None,
        ),
        // Online by unexpired membership + endpoint: spared.
        row(
            "online-registry",
            Some("Workstation"),
            NOW - 100,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW + 1800),
        ),
        // Live-connected peer even with stale endpoint metadata: spared.
        row(
            "online-live",
            Some("Cloud-Box"),
            NOW - 20 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 10),
            None,
        ),
        // Recently signed out: selected under default mode.
        row(
            "signed-out-recent",
            Some("Laptop"),
            NOW - 3600,
            true,
            RegistryPresence::SignedOut,
            Some(NOW + 3600),
            None,
        ),
        // Expired membership: selected under default mode.
        row(
            "expired-recent",
            Some("Tablet"),
            NOW - 7200,
            true,
            RegistryPresence::Present,
            Some(NOW - 60),
            None,
        ),
        // Unreachable for 2 days (< 7d default): spared.
        row(
            "unreachable-recent",
            Some("Brief-Offline"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW - 2 * SECONDS_PER_DAY),
        ),
        // Unreachable for 10 days (>= 7d default): selected.
        row(
            "unreachable-old",
            Some("Abandoned"),
            NOW - 10 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            None,
            None,
        ),
    ];
    let live: HashSet<String> = ["online-live".to_owned()].into_iter().collect();

    let selected = select_stale_installations(&rows, "self-id", &live, NOW, None);
    let summary: Vec<(&str, StaleReason)> = selected
        .iter()
        .map(|c| (c.row.installation_id.as_str(), c.reason))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("unreachable-old", StaleReason::Unreachable),
            ("signed-out-recent", StaleReason::SignedOut),
            ("expired-recent", StaleReason::MembershipExpired),
        ]
    );
}

#[test]
fn older_than_days_filters_all_stale_categories_by_age_and_never_selects_online_or_self() {
    let rows = vec![
        row(
            "self-id",
            Some("Self"),
            NOW - 60 * SECONDS_PER_DAY,
            true,
            RegistryPresence::SignedOut,
            None,
            None,
        ),
        row(
            "online-id",
            Some("Online"),
            NOW - 60 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW + 3600),
        ),
        row(
            "signed-out-2d",
            Some("Recent-Out"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::SignedOut,
            None,
            None,
        ),
        row(
            "expired-30d",
            Some("Old-Expired"),
            NOW - 35 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 30 * SECONDS_PER_DAY),
            None,
        ),
        row(
            "unreachable-2d",
            Some("Offline-2d"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW - 2 * SECONDS_PER_DAY),
        ),
    ];
    let live = HashSet::new();

    // --older-than 14 only keeps the 30-day-old expired machine.
    let older_14 = select_stale_installations(&rows, "self-id", &live, NOW, Some(14));
    assert_eq!(
        older_14
            .iter()
            .map(|c| c.row.installation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["expired-30d"]
    );

    // --older-than 0 includes every non-online stale machine, still sparing self and online.
    let older_0 = select_stale_installations(&rows, "self-id", &live, NOW, Some(0));
    assert_eq!(
        older_0
            .iter()
            .map(|c| c.row.installation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["expired-30d", "signed-out-2d", "unreachable-2d"]
    );
}
