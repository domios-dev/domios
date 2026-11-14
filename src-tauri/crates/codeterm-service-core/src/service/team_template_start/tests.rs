use codeterm_team_package::contracts::{TemplateRef, TemplateRuntimeNeed};

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
    let roles: Vec<_> = preview
        .form
        .pickers
        .iter()
        .map(|picker| picker.role.as_str())
        .collect();
    assert_eq!(roles, vec!["orchestrator", "planner"]);
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
        .partition(|role| role.kind == RoleKind::Worker);
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
        .map(|picker| picker.role.as_str())
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
fn builtin_permissions_remain_visible_without_consent_or_a_decision() {
    let ledger = ConsentLedger::default();
    for name in ["design-studio", "video-studio", "marketing-studio"] {
        let preview = preview_on(&template(name), &TargetCatalogue::default(), &ledger).unwrap();
        assert!(!preview.asks.is_empty(), "{name}");
        assert!(preview.consent.is_none(), "{name}");
        let resolved = resolve(Some(&template(name))).unwrap();
        let mut request: TemplateStartRequest = serde_json::from_value(serde_json::json!({
            "template": { "name": name }
        }))
        .unwrap();
        for field in input_fields(&resolved.package) {
            request.inputs.insert(field.key, "Fixture input".into());
        }
        assert!(admit(Some(&request), &ledger).is_ok(), "{name}");
    }
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

fn package(text: &str) -> codeterm_team_package::Package {
    codeterm_team_package::check_yaml(text, &codeterm_team_package::CheckOptions::default())
        .package
        .expect("fixture package is valid ATP")
}

const HEAD: &str = "name: dev.example/probe\nversion: 1.0.0\ndescription: Probe package.\nroles:\n  orchestrator:\n    kind: manager\n    instructions: Lead.\n";

fn with_role(name: &str, body: &str) -> codeterm_team_package::Package {
    package(&format!("{HEAD}  {name}:\n    kind: manager\n{body}"))
}

#[test]
fn a_schedule_on_any_role_but_the_curator_is_refused_by_this_host() {
    let scheduled = "    start:\n      schedule: 45m..2h\n    instructions: Fixture.\n";
    assert_eq!(
        resolve::host_policy(&with_role("auditor", scheduled)),
        Err(TemplateStartRefusal::ScheduleUnsupportedRole {
            role: "auditor".into()
        })
    );
    assert_eq!(
        resolve::host_policy(&with_role("curator", scheduled)),
        Ok(())
    );
}

#[test]
fn host_owned_names_are_refused_as_package_roles() {
    for name in ["watcher", "general_agent"] {
        assert_eq!(
            resolve::host_policy(&with_role(name, "    instructions: Fixture.\n")),
            Err(TemplateStartRefusal::HostOwnedRole { role: name.into() })
        );
    }
}

/// Start resolves through this policy before reserving anything, so the refusal is typed and
/// no tab or Team exists yet.
#[test]
fn a_domios_worker_kind_declared_as_a_manager_is_refused_before_start() {
    for name in ["qa", "reviewer", "release_manager"] {
        assert_eq!(
            resolve::host_policy(&with_role(name, "    instructions: Fixture.\n")),
            Err(TemplateStartRefusal::HostOwnedRole { role: name.into() })
        );
        let as_worker = package(&format!(
            "{HEAD}  {name}:\n    kind: worker\n    instructions: Fixture.\n"
        ));
        assert_eq!(resolve::host_policy(&as_worker), Ok(()), "{name}");
    }
}

#[test]
fn custom_with_team_managers_get_pickers_keyed_by_role_name() {
    let text = format!(
        "{HEAD}  security_lead:\n    kind: manager\n    name: Warden\n    start: with_team\n    instructions: Review.\n  helper:\n    kind: manager\n    instructions: Help.\n"
    );
    let resolved = ResolvedTemplate {
        package: package(&text),
        provenance: codeterm_team_package::contracts::TemplateProvenance::Saved,
        warnings: Vec::new(),
    };
    assert_eq!(
        with_team_managers(&resolved),
        ["orchestrator", "security_lead"]
    );
}
