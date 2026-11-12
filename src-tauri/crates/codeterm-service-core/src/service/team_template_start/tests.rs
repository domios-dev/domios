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
    assert_eq!(
        preview.form.template.name,
        resolve(None).unwrap().package.document.name
    );
    let slots: Vec<_> = preview
        .form
        .pickers
        .iter()
        .map(|picker| picker.slot)
        .collect();
    assert_eq!(slots, vec![ManagerSlot::Orchestrator, ManagerSlot::Planner]);
    assert!(preview.consent.is_none());
    assert!(preview.asks.is_empty());
}

#[test]
fn preview_roles_carry_their_model_decision_unless_a_picker_does() {
    let ledger = ConsentLedger::default();
    let preview = preview_on(&template("default"), &TargetCatalogue::default(), &ledger).unwrap();
    let (workers, managers): (Vec<_>, Vec<_>) = preview
        .form
        .roles
        .iter()
        .partition(|role| role.group == TeamRoleGroup::Worker);
    assert!(!workers.is_empty());
    for worker in workers {
        let decision = worker
            .model
            .as_ref()
            .expect("a worker carries its decision");
        assert_eq!(
            decision.reason,
            codeterm_team_package::contracts::ModelDecisionReason::NormalDefault,
            "an undeclared worker model is the owner's pick"
        );
        assert!(decision.choice.is_none());
    }
    let pickers: Vec<&str> = preview
        .form
        .pickers
        .iter()
        .map(|picker| picker.slot.as_str())
        .collect();
    for manager in managers {
        assert_eq!(
            manager.model.is_none(),
            pickers.contains(&manager.role.as_str()),
            "{} carries its decision only when it has no picker",
            manager.role
        );
    }
    let curator = preview
        .form
        .roles
        .iter()
        .find(|role| role.role == "curator")
        .expect("the default team has a curator");
    assert_eq!(curator.same_as.as_deref(), Some("orchestrator"));
}

#[test]
fn a_preview_issues_the_decision_its_start_must_carry() {
    let ledger = ConsentLedger::default();
    let preview = preview_on(
        &template("release-train"),
        &TargetCatalogue::default(),
        &ledger,
    )
    .unwrap();
    let consent = preview.consent.expect("declared permissions need consent");
    assert_eq!(consent.reasons, vec![ConsentReason::NewGrants]);
    let identity = resolve(Some(&template("release-train")))
        .unwrap()
        .identity();
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
        let needs = preview
            .where_needed
            .map(|place| place.needs)
            .unwrap_or_default();
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

#[test]
fn a_binding_to_an_absent_vault_entry_refuses_as_a_missing_input() {
    let mut start = TemplateStartRequest {
        template: template("default"),
        team_id: None,
        inputs: Default::default(),
        opt_out: Vec::new(),
        decision_id: None,
        secret_bindings: [("TOKEN".to_string(), "entry-opaque".to_string())].into(),
        manager_models: Default::default(),
        role_placements: Default::default(),
    };
    let ledger = ConsentLedger::default();
    let present = |entry: &str| -> Result<bool, String> { Ok(entry == "entry-present") };
    let unbound = super::super::team_template_remote::unbound_secrets(&start, present);
    assert_eq!(
        admit_bound(Some(&start), &ledger, &unbound).unwrap_err(),
        TemplateStartRefusal::MissingInputs {
            inputs: vec!["secret:TOKEN".into()]
        }
    );
    start
        .secret_bindings
        .insert("TOKEN".into(), "entry-present".into());
    let unbound = super::super::team_template_remote::unbound_secrets(&start, present);
    assert!(admit_bound(Some(&start), &ledger, &unbound).is_ok());
}
