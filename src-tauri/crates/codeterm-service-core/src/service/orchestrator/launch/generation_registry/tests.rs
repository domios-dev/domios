use super::*;

#[test]
fn explicit_restart_cancels_restored_worker_before_spawn() {
    let mut registry = LaunchRegistry::default();
    let mut restored = registry.begin("tab", "pending".into());
    registry.supersede("tab");
    let mut restart = registry.begin("tab", "pending".into());
    let generation = restart.generation;

    assert!(!registry.owns(&restored));
    assert!(!registry.replace_pty(&mut restored, "restored-candidate".into()));
    assert!(registry.replace_pty(&mut restart, "fresh-pty".into()));
    assert_eq!(restart.generation, generation);
    assert!(!registry.finish(&restored));
    assert!(registry.owns(&restart));
    assert!(registry.bind_provider_session(&restart, "fresh-nonce".into()));
    assert!(registry.record_provider_session(
        "tab",
        &ProviderSessionBinding {
            pty_id: "fresh-pty".into(),
            binding_nonce: "fresh-nonce".into(),
        },
    ));
    assert!(*restart.provider_ack.ready.lock());
    assert!(registry.finish(&restart));
}

#[test]
fn explicit_restart_supersedes_already_linked_restored_pty() {
    let mut registry = LaunchRegistry::default();
    let mut restored = registry.begin("tab", "pending".into());
    let generation = restored.generation;
    assert!(registry.replace_pty(&mut restored, "restored-pty".into()));
    assert_eq!(restored.generation, generation);
    assert!(registry.bind_provider_session(&restored, "restored-nonce".into()));

    registry.supersede("tab");
    let mut restart = registry.begin("tab", "pending".into());
    assert!(registry.take_for_pty("restored-pty").is_none());
    assert!(!registry.replace_pty(&mut restored, "late-restored-pty".into()));
    assert!(registry.replace_pty(&mut restart, "fresh-pty".into()));
    assert!(!registry.bind_provider_session(&restored, "late-nonce".into()));
    assert!(!registry.record_provider_session(
        "tab",
        &ProviderSessionBinding {
            pty_id: "restored-pty".into(),
            binding_nonce: "restored-nonce".into(),
        },
    ));
    assert!(registry.take_for_pty("restored-pty").is_none());
    assert!(registry.owns(&restart));
    assert!(registry.bind_provider_session(&restart, "fresh-nonce".into()));
    assert!(registry.record_provider_session(
        "tab",
        &ProviderSessionBinding {
            pty_id: "fresh-pty".into(),
            binding_nonce: "fresh-nonce".into(),
        },
    ));
    assert!(*restart.provider_ack.ready.lock());
    assert!(registry.finish(&restart));
}

#[test]
fn replacing_pty_keeps_generation_but_requires_its_own_acknowledgement() {
    let mut registry = LaunchRegistry::default();
    let mut launch = registry.begin("tab", "first-pty".into());
    let generation = launch.generation;
    assert!(registry.bind_provider_session(&launch, "first-nonce".into()));
    assert!(registry.record_provider_session(
        "tab",
        &ProviderSessionBinding {
            pty_id: "first-pty".into(),
            binding_nonce: "first-nonce".into(),
        },
    ));
    assert!(registry.replace_pty(&mut launch, "replacement-pty".into()));
    assert_eq!(launch.generation, generation);
    assert!(!*launch.provider_ack.ready.lock());
    assert!(launch.binding_nonce.is_none());
    assert!(registry.take_for_pty("first-pty").is_none());
    assert!(registry.owns(&launch));
}

#[test]
fn restart_owns_one_linked_pty_in_either_worker_order() {
    use crate::service::pty_ownership::{LinkOutcome, PtyOwnership};
    use std::cell::RefCell;
    use std::collections::HashSet;

    for restored_worker_first in [false, true] {
        let ownership = PtyOwnership::default();
        let claim = ownership.acquire("ordered-restart");
        let live = RefCell::new(HashSet::<String>::new());
        let linked = RefCell::new(None::<String>);
        let reaped = RefCell::new(Vec::<String>::new());
        let mut registry = LaunchRegistry::default();
        let mut restored = registry.begin("ordered-restart", "pending".into());
        let settle = |candidate: &str| {
            live.borrow_mut().insert(candidate.into());
            claim.settle(
                candidate.into(),
                true,
                |id| live.borrow().contains(id),
                |id| LinkOutcome::Linked(linked.replace(Some(id.into()))),
                |id| {
                    assert!(live.borrow_mut().remove(id));
                    reaped.borrow_mut().push(id.into());
                },
                |_| {},
            )
        };
        if restored_worker_first {
            assert!(registry.replace_pty(&mut restored, "restored-pty".into()));
            assert!(settle("restored-pty").retained_candidate);
        }
        registry.supersede("ordered-restart");
        if let Some(previous) = linked.borrow_mut().take() {
            ownership.release_pty(&previous);
            assert!(live.borrow_mut().remove(&previous));
            reaped.borrow_mut().push(previous);
        }
        let mut restart = registry.begin("ordered-restart", "pending".into());
        assert!(!registry.replace_pty(&mut restored, "late-restored-pty".into()));
        assert!(registry.replace_pty(&mut restart, "fresh-pty".into()));
        let selection = settle("fresh-pty");
        assert!(selection.linked && selection.retained_candidate);
        assert_eq!(linked.borrow().as_deref(), Some("fresh-pty"));
        assert_eq!(*live.borrow(), HashSet::from(["fresh-pty".into()]));
        assert_eq!(
            *reaped.borrow(),
            if restored_worker_first {
                vec!["restored-pty".to_string()]
            } else {
                Vec::new()
            }
        );
        assert!(registry.take_for_pty("restored-pty").is_none());
        assert!(registry.owns(&restart));
        assert!(registry.bind_provider_session(&restart, "fresh-nonce".into()));
        assert!(registry.record_provider_session(
            "ordered-restart",
            &ProviderSessionBinding {
                pty_id: selection.pty_id,
                binding_nonce: "fresh-nonce".into(),
            },
        ));
        assert!(*restart.provider_ack.ready.lock());
        assert!(registry.finish(&restart));
    }
}
