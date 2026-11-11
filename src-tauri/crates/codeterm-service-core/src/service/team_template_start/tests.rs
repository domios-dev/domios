use codeterm_team_package::contracts::{ConsentReason, TemplateRef, TemplateRuntimeNeed};

use super::*;

fn template(name: &str) -> TemplateRef {
    TemplateRef {
        name: name.into(),
        version: None,
    }
}

#[test]
fn the_default_preview_asks_nothing_and_offers_both_pair_pickers() {
    let ledger = ConsentLedger::default();
    let preview = preview_on(&template("default"), &TargetCatalogue::default(), &ledger).unwrap();
    assert_eq!(preview.form.template.name, resolve(None).unwrap().package.document.name);
    let slots: Vec<_> = preview.form.pickers.iter().map(|picker| picker.slot).collect();
    assert_eq!(slots, vec![ManagerSlot::Orchestrator, ManagerSlot::Planner]);
    assert!(preview.consent.is_none());
    assert!(preview.asks.is_empty());
}

#[test]
fn a_preview_issues_the_decision_its_start_must_carry() {
    let ledger = ConsentLedger::default();
    let preview =
        preview_on(&template("release-train"), &TargetCatalogue::default(), &ledger).unwrap();
    let consent = preview.consent.expect("declared permissions need consent");
    assert_eq!(consent.reasons, vec![ConsentReason::NewGrants]);
    let identity = resolve(Some(&template("release-train"))).unwrap().identity();
    assert!(ledger.admits(Some(&consent.decision_id), &identity, &consent.reasons));
    assert!(!ledger.admits(Some("consent_forged"), &identity, &consent.reasons));
    assert!(!ledger.admits(None, &identity, &consent.reasons));
}

#[test]
fn required_harnesses_are_listed_and_absent_ones_are_needs() {
    let ledger = ConsentLedger::default();
    for resolved in builtin_templates() {
        let preview = preview_on(
            &template(&resolved.package.document.name),
            &TargetCatalogue::default(),
            &ledger,
        )
        .unwrap();
        assert_eq!(preview.requires_harnesses, resolved.requires_harnesses());
        let needs = preview.where_needed.map(|place| place.needs).unwrap_or_default();
        assert_eq!(needs.len(), preview.requires_harnesses.len());
        assert!(needs
            .iter()
            .all(|need| matches!(need, TemplateRuntimeNeed::MissingHarness { .. })));
    }
}

#[test]
fn a_version_pin_must_match_the_builtin() {
    let resolved = resolve(None).unwrap();
    let pinned = TemplateRef {
        name: resolved.package.document.name.clone(),
        version: Some(resolved.package.document.version.clone()),
    };
    assert!(resolve(Some(&pinned)).is_ok());
    let other = TemplateRef {
        version: Some("0.0.0-other".into()),
        ..pinned
    };
    assert!(matches!(
        resolve(Some(&other)),
        Err(TemplateStartRefusal::NotFound { .. })
    ));
}
