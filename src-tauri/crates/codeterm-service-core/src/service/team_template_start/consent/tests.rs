use super::*;
use crate::service::team_template_start::{admit_resolved, resolve, ResolvedTemplate};
use codeterm_team_package::contracts::{TemplateRef, TemplateStartRefusal, TemplateStartRequest};

fn package(asks: bool) -> Package {
    let name = if asks { "design-studio" } else { "default" };
    resolve(Some(&TemplateRef {
        name: name.into(),
        version: None,
    }))
    .unwrap()
    .package
}

#[test]
fn only_imported_packages_require_consent_with_or_without_asks() {
    for asks in [false, true] {
        let package = package(asks);
        for provenance in [TemplateProvenance::Builtin, TemplateProvenance::Saved] {
            assert!(consent_reasons(&package, provenance).is_empty());
        }
        let mut expected = vec![ConsentReason::UnverifiedPackage];
        if asks {
            expected.push(ConsentReason::NewGrants);
        }
        assert_eq!(
            consent_reasons(&package, TemplateProvenance::Imported),
            expected
        );
    }
}

fn resolved(package: Package) -> ResolvedTemplate {
    ResolvedTemplate {
        package,
        provenance: TemplateProvenance::Imported,
        warnings: Vec::new(),
    }
}

fn request(package: &Package) -> TemplateStartRequest {
    let mut request: TemplateStartRequest = serde_json::from_value(serde_json::json!({
        "template": { "name": package.document.name }
    }))
    .unwrap();
    for field in codeterm_team_package::contracts::input_fields(package) {
        request.inputs.insert(field.key, "Fixture input".into());
    }
    request
}

#[test]
fn imported_consent_survives_restart_and_binds_every_part_of_the_package() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ConsentLedger::persistent(directory.path().into());
    let package = package(true);
    let reasons = consent_reasons(&package, TemplateProvenance::Imported);
    let mut request = request(&package);
    assert!(!ledger.accepted(&package));
    assert!(matches!(
        admit_resolved(resolved(package.clone()), Some(&request), &ledger, &[]),
        Err(TemplateStartRefusal::ConsentRequired { .. })
    ));
    let decision = ledger.issue(&package, &reasons);
    assert!(!ledger.accepted(&package), "preview alone is not consent");
    request.decision_id = Some("consent_forged".into());
    assert!(admit_resolved(resolved(package.clone()), Some(&request), &ledger, &[]).is_err());
    request.decision_id = Some(decision.clone());
    assert!(admit_resolved(resolved(package.clone()), Some(&request), &ledger, &[]).is_ok());
    drop(ledger);
    let restarted = ConsentLedger::persistent(directory.path().into());
    assert!(restarted.accepted(&package));
    request.decision_id = None;
    assert!(admit_resolved(resolved(package.clone()), Some(&request), &restarted, &[]).is_ok());
    let current_decision = restarted.issue(&package, &reasons);
    request.decision_id = Some(current_decision.clone());
    for change in ["name", "version", "instructions", "file"] {
        let mut changed = package.clone();
        match change {
            "name" => changed.document.name = "example/other".into(),
            "version" => changed.document.version = "99.0.0".into(),
            "instructions" => changed.document.description = "Changed content".into(),
            _ => {
                changed
                    .entries
                    .insert("instructions.md".into(), b"Changed file".to_vec());
            }
        }
        assert!(!restarted.accepted(&changed), "{change}");
        assert!(!restarted.admits(Some(&current_decision), &changed, &reasons));
        assert!(
            matches!(
                admit_resolved(resolved(changed.clone()), Some(&request), &restarted, &[]),
                Err(TemplateStartRefusal::ConsentRequired { .. })
            ),
            "{change}"
        );
    }
}

#[test]
fn unreadable_consent_records_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let package = package(false);
    std::fs::write(directory.path().join(identity(&package)), b"broken").unwrap();
    let ledger = ConsentLedger::persistent(directory.path().into());
    assert!(!ledger.accepted(&package));
}

#[test]
fn locally_saved_templates_start_without_consent_even_with_asks() {
    let ledger = ConsentLedger::default();
    for asks in [false, true] {
        let package = package(asks);
        let request = request(&package);
        let mut template = resolved(package);
        template.provenance = TemplateProvenance::Saved;
        let admitted = admit_resolved(template, Some(&request), &ledger, &[]).unwrap();
        assert!(admitted.decision_id.is_none());
    }
}

#[test]
fn failure_to_persist_acceptance_does_not_admit_an_import() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("not-a-directory");
    std::fs::write(&root, b"occupied").unwrap();
    let ledger = ConsentLedger::persistent(root);
    let package = package(false);
    let mut request = request(&package);
    request.decision_id = Some(ledger.issue(
        &package,
        &consent_reasons(&package, TemplateProvenance::Imported),
    ));
    assert!(matches!(
        admit_resolved(resolved(package), Some(&request), &ledger, &[]),
        Err(TemplateStartRefusal::ConsentRequired { .. })
    ));
}

#[test]
fn community_consent_uses_the_same_durable_exact_package_record() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = ConsentLedger::persistent(directory.path().into());
    let package = package(false);
    let provenance = TemplateProvenance::Channel;
    let reasons = consent_reasons(&package, provenance);
    assert_eq!(reasons, vec![ConsentReason::UnverifiedPackage]);
    let mut request = request(&package);
    let mut template = resolved(package.clone());
    template.provenance = provenance;
    assert!(matches!(
        admit_resolved(template.clone(), Some(&request), &ledger, &[]),
        Err(TemplateStartRefusal::ConsentRequired { .. })
    ));
    request.decision_id = Some(ledger.issue(&package, &reasons));
    admit_resolved(template.clone(), Some(&request), &ledger, &[]).unwrap();
    let reopened = ConsentLedger::persistent(directory.path().into());
    request.decision_id = None;
    admit_resolved(template.clone(), Some(&request), &reopened, &[]).unwrap();
    for change_version in [true, false] {
        let mut changed = template.clone();
        if change_version {
            changed.package.document.version = "99.0.0".into();
        } else {
            changed.package.document.description.push_str(" changed");
        }
        assert!(matches!(
            admit_resolved(changed, Some(&request), &reopened, &[]),
            Err(TemplateStartRefusal::ConsentRequired { .. })
        ));
    }
}
