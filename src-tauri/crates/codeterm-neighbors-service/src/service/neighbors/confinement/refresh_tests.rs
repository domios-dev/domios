use super::*;
use crate::prelude::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
    service::test_fixture::ServiceFixture,
};

#[test]
fn account_refresh_keeps_root_and_child_authorized_until_atomic_publication() {
    for access in [
        MachineAccess::ReadOnly,
        MachineAccess::Folder,
        MachineAccess::Full,
    ] {
        let (mut mux, root) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&root).unwrap();
        let child = mux.create_tab(&workspace).unwrap();
        let foreign = mux.create_tab(&workspace).unwrap();
        let friend = "a".repeat(64);
        let other_friend = "b".repeat(64);
        for (id, uid, friendship, parent) in [
            (&root, "owner", &friend, None),
            (&child, "owner", &friend, Some(root.clone())),
            (&foreign, "other-owner", &other_friend, None),
        ] {
            let tab = mux.get_tab_mut(id).unwrap();
            tab.session_binding_nonce = Some(format!("nonce-{id}"));
            tab.neighbor_authority = Some(NeighborAuthority {
                uid: uid.into(),
                friendship_id: friendship.clone(),
                root_tab_id: parent,
            });
        }
        let fixture = crate::test_support::fixture(mux);
        let svc = &fixture.service;
        for friendship in [&friend, &other_friend] {
            svc.set_neighbor_friend_receiver_for_test(friendship, Some(true), 1);
        }
        let mut settings = Settings::default();
        settings.bindings.insert(friend.clone(), root.clone());
        settings.permissions.insert(
            friend.clone(),
            Permission {
                machine_access: access,
                ..Default::default()
            },
        );
        svc.remember_neighbor_concierge_accounts("owner", &settings);
        svc.remember_neighbor_restore_grant(&foreign, MachineAccess::Full);
        let bypass = access == MachineAccess::Full;
        let check = || {
            assert_eq!(svc.neighbor_restore_access(&root).unwrap(), access);
            for tab in [&root, &child] {
                assert!(svc.require_neighbor_restore_grant(tab, bypass).is_ok());
                assert!(svc.require_neighbor_restore_grant(tab, !bypass).is_err());
            }
            assert!(svc.require_neighbor_restore_grant(&foreign, true).is_ok());
        };
        svc.refresh_neighbor_restore_grants("owner", &settings, check);
        check();

        settings
            .permissions
            .get_mut(&friend)
            .unwrap()
            .machine_access = MachineAccess::ReadOnly;
        svc.refresh_neighbor_restore_grants("owner", &settings, check);
        for tab in [&root, &child] {
            assert!(svc.require_neighbor_restore_grant(tab, false).is_ok());
            assert!(svc.require_neighbor_restore_grant(tab, true).is_err());
        }
        assert_eq!(
            svc.neighbor_restore_access(&root).unwrap(),
            MachineAccess::ReadOnly
        );
        svc.refresh_neighbor_restore_grants("owner", &Settings::default(), || {
            assert!(svc.require_neighbor_restore_grant(&root, false).is_ok());
        });
        assert_eq!(
            svc.require_neighbor_restore_grant(&root, false)
                .unwrap_err(),
            codeterm_neighbors::Error::RestoreGrantRequired.to_string()
        );
        assert!(svc.require_neighbor_restore_grant(&child, false).is_err());
        assert!(svc.require_neighbor_restore_grant(&foreign, true).is_ok());
    }
}

#[test]
fn account_refresh_cannot_retag_foreign_roots_or_bypass_receiver_and_nonce_checks() {
    let (mut mux, root) = MultiplexerState::new();
    let friend = "a".repeat(64);
    let tab = mux.get_tab_mut(&root).unwrap();
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.neighbor_authority = Some(NeighborAuthority {
        uid: "owner".into(),
        friendship_id: friend.clone(),
        root_tab_id: None,
    });
    let fixture = crate::test_support::fixture(mux);
    let svc = &fixture.service;
    svc.set_neighbor_friend_receiver_for_test(&friend, Some(true), 1);
    let mut settings = Settings::default();
    settings.bindings.insert(friend.clone(), root.clone());
    settings
        .permissions
        .insert(friend.clone(), Permission::default());
    svc.remember_neighbor_concierge_accounts("owner", &settings);
    svc.refresh_neighbor_restore_grants("foreign-owner", &settings, || {});
    assert!(svc.require_neighbor_restore_grant(&root, false).is_ok());
    assert!(svc.require_neighbor_restore_grant(&root, true).is_err());
    svc.mux
        .write()
        .get_tab_mut(&root)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    assert!(svc.require_neighbor_restore_grant(&root, false).is_err());
    svc.remember_neighbor_concierge_accounts("owner", &settings);
    assert!(svc.require_neighbor_restore_grant(&root, false).is_ok());
    svc.set_neighbor_friend_receiver_for_test(&friend, Some(false), 2);
    svc.remember_neighbor_concierge_accounts("owner", &settings);
    assert!(svc.require_neighbor_restore_grant(&root, false).is_err());
    svc.set_neighbor_friend_receiver_for_test(&friend, Some(true), 3);
    svc.remember_neighbor_concierge_accounts("owner", &settings);
    svc.refresh_neighbor_restore_grants("owner", &settings, || {
        svc.neighbor_cell()
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        svc.set_neighbor_friend_receiver_for_test(&friend, Some(true), 3);
    });
    assert!(svc.require_neighbor_restore_grant(&root, false).is_err());
}
