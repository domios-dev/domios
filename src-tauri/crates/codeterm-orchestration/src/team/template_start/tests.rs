use std::collections::BTreeMap;
use std::sync::Arc;

use codeterm_service_core::agent::provider::ModelInfo;
use codeterm_service_core::service::model_decision::{
    HarnessCatalogue, HarnessReadiness, TargetCatalogue,
};
use codeterm_service_core::service::team_roles::TeamRoleStore;
use codeterm_service_core::service::team_template_start::{
    admit, admit_resolved, resolve, AdmittedTemplate, ConsentLedger, ResolvedTemplate,
};
use codeterm_team_package::contracts::{
    ManagerStartFailure, ModelBlockedReason, TemplateItemState, TemplateProvenance, TemplateRef,
    TemplateStartRefusal, TemplateStartRequest, TemplateStartResult,
};
use codeterm_team_package::{
    check_archive, check_entries, check_yaml, CheckOptions, DiagnosticCode, PackageFormat,
    RoleKind, RoleStart, StartMode,
};

use super::registration::{register_all, registrations_for};
use super::rerun::missing_manager;
use super::seats::{plan_seats, OwnerPick, SeatPlan};
use super::summary::{pinned_statuses, summary_text, PostStartStatus};
use super::{payload_request, TemplateRail};
use crate::{OrchestrationPairExt, OrchestrationPairParams};

const TEAM: &str = "team_template_probe";

fn default_admitted() -> AdmittedTemplate {
    admit(None, &ConsentLedger::default()).expect("the builtin default starts without asks")
}

fn request(name: &str) -> TemplateStartRequest {
    TemplateStartRequest {
        template: TemplateRef {
            name: name.into(),
            version: None,
        },
        team_id: None,
        inputs: BTreeMap::new(),
        opt_out: Vec::new(),
        decision_id: None,
        secret_bindings: BTreeMap::new(),
        manager_models: BTreeMap::new(),
    }
}

fn rail(admitted: AdmittedTemplate, opt_out: &[&str]) -> TemplateRail {
    TemplateRail {
        admitted,
        opt_out: opt_out.iter().map(|id| id.to_string()).collect(),
        seats: BTreeMap::new(),
    }
}

fn no_endpoints(_: &str, _: &str) -> Option<Result<(), Vec<String>>> {
    None
}

fn probe_target(enforced_models: &[&str]) -> TargetCatalogue {
    let mut target = TargetCatalogue::default();
    target.harnesses.insert(
        "probe".into(),
        HarnessCatalogue {
            readiness: HarnessReadiness::Ready,
            models: enforced_models
                .iter()
                .map(|id| ModelInfo {
                    id: (*id).into(),
                    display_name: (*id).into(),
                    ..Default::default()
                })
                .collect(),
            enforced: true,
            efforts: Vec::new(),
            default_effort: None,
            default_model: None,
        },
    );
    target
}

#[test]
fn the_default_package_registers_every_role_pinned_and_starts_no_worker() {
    let admitted = default_admitted();
    let identity = admitted.resolved.identity();
    let registrations = registrations_for(TEAM, &admitted, 1);
    let starts: Vec<(&str, RoleStart)> = registrations
        .iter()
        .map(|registration| (registration.name.as_str(), registration.start.clone()))
        .collect();
    let on_demand = RoleStart::Mode(StartMode::OnDemand);
    assert_eq!(
        starts,
        vec![
            ("orchestrator", RoleStart::Mode(StartMode::WithTeam)),
            ("planner", RoleStart::Mode(StartMode::WithTeam)),
            ("cartographer", on_demand.clone()),
            (
                "curator",
                RoleStart::Schedule {
                    schedule: "45m..2h".into()
                }
            ),
            ("worker", on_demand.clone()),
            ("qa", on_demand.clone()),
            ("reviewer", on_demand.clone()),
            ("release_manager", on_demand),
        ]
    );
    assert!(registrations
        .iter()
        .all(|registration| registration.package == identity
            && !registration.instructions.trim().is_empty()));
    assert_eq!(
        super::seats::started_managers(&admitted.resolved),
        ["orchestrator", "planner"]
    );
    assert!(rail(admitted, &[]).starts_planner());
}

#[test]
fn re_running_registration_reports_present_items_and_keeps_other_versions() {
    let root = tempfile::tempdir().unwrap();
    let store = TeamRoleStore::new(root.path());
    let admitted = default_admitted();
    let first = register_all(&store, registrations_for(TEAM, &admitted, 1)).unwrap();
    assert!(first
        .iter()
        .all(|item| item.state == TemplateItemState::Created));

    // A fresh consent decision or clock must not turn the same package into a conflict.
    let mut again = admitted.clone();
    again.decision_id = Some("consent_later".into());
    let second = register_all(&store, registrations_for(TEAM, &again, 2)).unwrap();
    assert!(second
        .iter()
        .all(|item| item.state == TemplateItemState::AlreadyPresent));

    let mut newer = registrations_for(TEAM, &admitted, 3);
    for registration in &mut newer {
        registration.package.version = "9.9.9".into();
    }
    let third = register_all(&store, newer).unwrap();
    assert!(third
        .iter()
        .all(|item| item.state == TemplateItemState::PinnedToOtherVersion));
    let stored = store.registrations(TEAM);
    assert_eq!(stored.len(), first.len());
    assert!(stored
        .iter()
        .all(|registration| registration.package == admitted.resolved.identity()));
    assert_eq!(pinned_statuses(&third).len(), third.len());
}

#[test]
fn concurrent_re_runs_create_each_registration_exactly_once() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TeamRoleStore::new(root.path()));
    let admitted = default_admitted();
    let runs: Vec<_> = (0..2)
        .map(|run| {
            let store = Arc::clone(&store);
            let registrations = registrations_for(TEAM, &admitted, run);
            std::thread::spawn(move || register_all(&store, registrations).unwrap())
        })
        .collect();
    let outcomes: Vec<_> = runs.into_iter().map(|run| run.join().unwrap()).collect();
    for index in 0..outcomes[0].len() {
        let created = outcomes
            .iter()
            .filter(|items| items[index].state == TemplateItemState::Created)
            .count();
        assert_eq!(created, 1, "{}", outcomes[0][index].role);
        assert!(outcomes
            .iter()
            .all(|items| items[index].state != TemplateItemState::PinnedToOtherVersion));
    }
    assert_eq!(store.registrations(TEAM).len(), outcomes[0].len());
}

const ASKING: &str = "name: dev.example/asking
version: 1.0.0
description: Probe package.
inputs:
  goal: opaque-goal
  tone:
    description: opaque-tone
    required: false
    default: opaque-default
secrets:
  - name: TOKEN
    purpose: opaque-purpose
roles:
  orchestrator:
    kind: manager
    instructions: opaque-orchestrator
";

#[test]
fn missing_inputs_and_secrets_refuse_before_consent_or_creation() {
    let resolved = ResolvedTemplate::from_checked(
        check_yaml(ASKING, &CheckOptions::default()),
        TemplateProvenance::Saved,
    )
    .expect("probe package admits");
    let refusal = admit_resolved(
        resolved,
        Some(&request("dev.example/asking")),
        &ConsentLedger::default(),
        &[],
    )
    .unwrap_err();
    assert_eq!(
        refusal,
        TemplateStartRefusal::MissingInputs {
            inputs: vec!["goal".into(), "secret:TOKEN".into()]
        },
        "inputs with a default are not asked, and the gate runs before consent"
    );
}

fn filled(name: &str) -> TemplateStartRequest {
    let resolved = resolve(Some(&request(name).template)).unwrap();
    let mut request = request(name);
    for (key, _) in resolved.package.document.inputs.iter() {
        request.inputs.insert(key.to_string(), "opaque".into());
    }
    for secret in &resolved.package.document.secrets {
        request
            .secret_bindings
            .insert(secret.name.clone(), "vault-entry".into());
    }
    request
}

#[test]
fn builtin_grants_start_without_a_decision_and_register_without_one() {
    let ledger = ConsentLedger::default();
    let start = filled("design-studio");
    let admitted = admit(Some(&start), &ledger).expect("builtin permissions need no consent");
    assert!(admitted.decision_id.is_none());
    assert!(registrations_for(TEAM, &admitted, 1)
        .iter()
        .all(|registration| registration.consent_decision_id.is_none()));
}

#[test]
fn an_unknown_template_is_not_found() {
    let refusal = admit(
        Some(&request("dev.example/probe")),
        &ConsentLedger::default(),
    )
    .unwrap_err();
    assert!(matches!(refusal, TemplateStartRefusal::NotFound { .. }));
}

#[test]
fn no_manager_pick_asks_for_a_choice_for_both_pair_managers() {
    let admitted = default_admitted();
    let SeatPlan::NeedsChoice(pickers) = plan_seats(
        &admitted.resolved,
        &BTreeMap::new(),
        &TargetCatalogue::default(),
        &no_endpoints,
    ) else {
        panic!("a package without models needs a choice");
    };
    let roles: Vec<_> = pickers.iter().map(|picker| picker.role.as_str()).collect();
    assert_eq!(roles, ["orchestrator", "planner"]);
}

#[test]
fn a_later_manager_pick_lands_on_its_registration_and_its_attach_uses_it() {
    let admitted = default_admitted();
    let target = probe_target(&["m1"]);
    let mut picks = BTreeMap::new();
    for name in ["orchestrator", "planner", "cartographer"] {
        picks.insert(
            name.to_string(),
            OwnerPick::new("probe", Some("m1"), None).unwrap(),
        );
    }
    let SeatPlan::Seats(mut seats) = plan_seats(&admitted.resolved, &picks, &target, &no_endpoints)
    else {
        panic!("every pick is eligible");
    };
    // The probe catalogue has no effort levels; the decided effort rides on the seat.
    seats.get_mut("cartographer").unwrap().effort = Some("high".into());
    let started = super::seats::started_managers(&admitted.resolved);
    assert!(
        !started.iter().any(|name| name == "cartographer"),
        "the Cartographer joins on demand, so the start never launches it"
    );
    let mut registrations = registrations_for(TEAM, &admitted, 1);
    super::registration::record_later_seats(&mut registrations, &seats, &started);
    let registered = |name: &str| {
        registrations
            .iter()
            .find(|registration| registration.name == name)
            .cloned()
            .unwrap()
    };
    let cartographer = registered("cartographer");
    assert_eq!(
        cartographer.model,
        Some(codeterm_team_package::ModelPreference::List(vec![
            "probe:m1".into()
        ]))
    );
    assert_eq!(cartographer.reasoning.as_deref(), Some("high"));
    assert_eq!(
        registered("orchestrator").model,
        registrations_for(TEAM, &admitted, 1)[0].model,
        "a started manager keeps its package registration"
    );

    let mut attach = crate::AddTeamMemberParams {
        provider: "orchestrators-own".into(),
        role: "cartographer".into(),
        title: None,
        task: None,
        model: None,
    };
    let effort = crate::team::staffing::adopt_registered_model(&mut attach, Some(&cartographer));
    assert_eq!(
        (
            attach.provider.as_str(),
            attach.model.as_deref(),
            effort.as_deref()
        ),
        ("probe", Some("m1"), Some("high"))
    );
    let mut explicit = crate::AddTeamMemberParams {
        model: Some("chosen".into()),
        ..attach.clone()
    };
    explicit.provider = "caller".into();
    assert_eq!(
        crate::team::staffing::adopt_registered_model(&mut explicit, Some(&cartographer)),
        None
    );
    assert_eq!(
        (explicit.provider.as_str(), explicit.model.as_deref()),
        ("caller", Some("chosen"))
    );
}

#[test]
fn owner_picks_go_through_the_decision_service() {
    let admitted = default_admitted();
    let target = probe_target(&["m1"]);
    let mut picks = BTreeMap::new();
    picks.insert(
        "orchestrator".to_string(),
        OwnerPick::new("probe", Some("m1"), None).unwrap(),
    );
    picks.insert(
        "planner".to_string(),
        OwnerPick::new("probe", Some("  "), None).unwrap(),
    );
    let SeatPlan::Seats(seats) = plan_seats(&admitted.resolved, &picks, &target, &no_endpoints)
    else {
        panic!("both picks are eligible");
    };
    assert_eq!(seats["orchestrator"].model.as_deref(), Some("m1"));
    // A bare provider keeps its launch default instead of promoting one to an explicit model.
    assert_eq!(seats["planner"].model, None);

    picks.insert(
        "orchestrator".to_string(),
        OwnerPick::new("probe", Some("m2"), None).unwrap(),
    );
    let SeatPlan::NeedsChoice(pickers) =
        plan_seats(&admitted.resolved, &picks, &target, &no_endpoints)
    else {
        panic!("an unknown model is refused, never replaced");
    };
    assert_eq!(pickers.len(), 1);
    assert!(matches!(
        pickers[0].decision.blocked,
        Some(ModelBlockedReason::UnknownModel { .. })
    ));
}

/// The probe harness owns the `gateway` endpoint, which serves `gateway/m9` only.
fn gateway_endpoint(provider: &str, model: &str) -> Option<Result<(), Vec<String>>> {
    (provider == "probe" && model.starts_with("gateway/")).then(|| {
        if model == "gateway/m9" {
            Ok(())
        } else {
            Err(vec!["gateway/m9".to_string()])
        }
    })
}

#[test]
fn a_custom_endpoint_model_is_accepted_as_spawn_validation_accepts_it() {
    let admitted = default_admitted();
    // The enforced harness catalogue does not list endpoint ids; the endpoint catalogue owns them.
    let target = probe_target(&["m1"]);
    let mut picks = BTreeMap::new();
    for (name, model) in [
        ("orchestrator", "gateway/m9"),
        ("planner", "probe:gateway/m9"),
    ] {
        picks.insert(
            name.to_string(),
            OwnerPick::new("probe", Some(model), Some("high")).unwrap(),
        );
    }
    let SeatPlan::Seats(seats) = plan_seats(&admitted.resolved, &picks, &target, &gateway_endpoint)
    else {
        panic!("a configured endpoint model seats its manager");
    };
    assert_eq!(seats["orchestrator"].model.as_deref(), Some("gateway/m9"));
    assert_eq!(seats["planner"].model.as_deref(), Some("probe:gateway/m9"));
    assert_eq!(seats["planner"].effort.as_deref(), Some("high"));

    picks.insert(
        "orchestrator".to_string(),
        OwnerPick::new("probe", Some("gateway/absent"), None).unwrap(),
    );
    let SeatPlan::NeedsChoice(pickers) =
        plan_seats(&admitted.resolved, &picks, &target, &gateway_endpoint)
    else {
        panic!("an id the endpoint does not serve is refused");
    };
    assert!(matches!(
        &pickers[0].decision.blocked,
        Some(ModelBlockedReason::UnknownModel { suggestions, .. })
            if suggestions == &vec!["probe:gateway/m9".to_string()]
    ));
}

#[test]
fn a_bare_provider_on_an_absent_harness_is_blocked() {
    let admitted = default_admitted();
    let mut picks = BTreeMap::new();
    for name in ["orchestrator", "planner"] {
        picks.insert(
            name.to_string(),
            OwnerPick::new("missing", None, None).unwrap(),
        );
    }
    let SeatPlan::NeedsChoice(pickers) = plan_seats(
        &admitted.resolved,
        &picks,
        &TargetCatalogue::default(),
        &no_endpoints,
    ) else {
        panic!("an absent harness cannot seat a manager");
    };
    assert!(pickers.iter().all(|picker| matches!(
        picker.decision.blocked,
        Some(ModelBlockedReason::AbsentHarness { .. })
    )));
}

#[test]
fn only_opted_in_payload_reaches_the_payload_leaf() {
    assert!(payload_request(&rail(default_admitted(), &[])).is_none());
    let resolved = resolve(Some(&request("video-studio").template)).unwrap();
    let items = codeterm_team_package::payload_items(&resolved.package);
    assert!(!items.is_empty(), "the fixture package brings payload");
    let skipped = items[0].id.clone();
    let admitted = AdmittedTemplate {
        resolved,
        inputs: BTreeMap::new(),
        decision_id: None,
    };
    let (_, opt_out) = payload_request(&rail(admitted, &[skipped.as_str()])).unwrap();
    assert_eq!(opt_out, vec![skipped]);
}

#[test]
fn post_start_failures_become_one_summary_with_a_line_per_role() {
    let ok = [
        PostStartStatus::ok("orchestrator", "started"),
        PostStartStatus::ok("planner", "started"),
    ];
    assert_eq!(summary_text(TEAM, &ok), None);
    let mixed = [
        PostStartStatus::ok("orchestrator", "started"),
        PostStartStatus::failed("cartographer", "opaque-reason-1"),
        PostStartStatus::failed("payload", "opaque-reason-2"),
    ];
    let text = summary_text(TEAM, &mixed).unwrap();
    assert!(text.contains(TEAM));
    for fragment in [
        "orchestrator",
        "cartographer",
        "opaque-reason-1",
        "opaque-reason-2",
    ] {
        assert_eq!(text.matches(fragment).count(), 1, "{fragment}: {text}");
    }
}

#[test]
fn a_rerun_reports_a_missing_manager_instead_of_replacing_it() {
    let names = ["orchestrator".to_string(), "planner".to_string()];
    let mut live = BTreeMap::new();
    live.insert("orchestrator".to_string(), "tab-o".to_string());
    let failure: ManagerStartFailure = missing_manager(TEAM, &names, &live).unwrap();
    assert_eq!(failure.role, "planner");
    live.insert("planner".to_string(), "tab-p".to_string());
    assert!(missing_manager(TEAM, &names, &live).is_none());
}

const SKILLED: &str = "name: dev.example/skilled
version: 1.0.0
description: Probe package.
roles:
  orchestrator:
    kind: manager
    instructions: opaque-orchestrator
  scout:
    kind: worker
    instructions: opaque-scout
    skills: [absent-skill]
";

#[test]
fn every_package_form_starts_identically_and_keeps_skill_warnings() {
    let options = CheckOptions::default();
    let yaml =
        ResolvedTemplate::from_checked(check_yaml(SKILLED, &options), TemplateProvenance::Saved)
            .expect("an unresolved skill is a warning, not a refusal");
    assert!(yaml
        .warnings
        .iter()
        .any(|warning| warning.code == DiagnosticCode::UnresolvedSkill));
    let archive = ResolvedTemplate::from_checked(
        check_archive(&yaml.package.to_archive().unwrap(), &options),
        TemplateProvenance::Saved,
    )
    .unwrap();
    let folder = ResolvedTemplate::from_checked(
        check_entries(
            PackageFormat::Folder,
            yaml.package.archive_entries(),
            Vec::new(),
            &options,
        ),
        TemplateProvenance::Saved,
    )
    .unwrap();
    let started: Vec<_> = [yaml, archive, folder]
        .into_iter()
        .map(|resolved| {
            let admitted = AdmittedTemplate {
                resolved,
                inputs: BTreeMap::new(),
                decision_id: None,
            };
            registrations_for(TEAM, &admitted, 1)
        })
        .collect();
    assert_eq!(started[0], started[1]);
    assert_eq!(started[0], started[2]);
    assert!(started[0]
        .iter()
        .any(|registration| registration.kind == RoleKind::Worker
            && registration.name == "scout"
            && registration.skills == vec!["absent-skill".to_string()]));

    let malformed = check_yaml("name: [", &options);
    assert!(matches!(
        ResolvedTemplate::from_checked(malformed, TemplateProvenance::Saved),
        Err(TemplateStartRefusal::InvalidPackage { .. })
    ));
}

#[test]
fn a_typed_outcome_survives_the_pair_error_string() {
    let result = TemplateStartResult::Refused {
        refusal: TemplateStartRefusal::MissingInputs {
            inputs: vec!["opaque".into()],
        },
    };
    assert_eq!(
        TemplateStartResult::from_error_message(&result.error_message()),
        Some(result)
    );
    assert_eq!(
        TemplateStartResult::from_error_message("opaque failure"),
        None
    );
}

#[test]
fn an_unknown_template_creates_nothing_through_the_pair_primitive() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    for provider in ["template-orchestrator", "template-planner"] {
        fixture.service.agent_registry.register_provider(Box::new(
            codeterm_service_core::agent::test_provider::MockProvider::new(provider),
        ));
    }
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "template-orchestrator".into(),
                    planner_agent: "template-planner".into(),
                    workspace_id: Some(workspace_id),
                    template: Some(request("dev.example/probe")),
                    ..Default::default()
                }),
        )
        .expect_err("an unknown template is refused before any creation");
    assert!(matches!(
        TemplateStartResult::from_error_message(&error),
        Some(TemplateStartResult::Refused {
            refusal: TemplateStartRefusal::NotFound { .. }
        })
    ));
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    assert!(fixture.service.plan_store.list_index().unwrap().is_empty());
}

fn saved(text: &str) -> AdmittedTemplate {
    AdmittedTemplate {
        resolved: ResolvedTemplate::from_checked(
            check_yaml(text, &CheckOptions::default()),
            TemplateProvenance::Saved,
        )
        .expect("fixture package admits"),
        inputs: BTreeMap::new(),
        decision_id: None,
    }
}

const ORCHESTRATOR_ONLY: &str = "name: dev.example/solo
version: 1.0.0
description: Probe package.
roles:
  orchestrator:
    kind: manager
    instructions: opaque-orchestrator
  scout:
    kind: worker
    instructions: opaque-scout
";

#[test]
fn an_orchestrator_only_package_seats_no_planner_and_has_no_curator_rounds() {
    let admitted = saved(ORCHESTRATOR_ONLY);
    assert_eq!(
        super::seats::started_managers(&admitted.resolved),
        ["orchestrator"]
    );
    let registrations = registrations_for(TEAM, &admitted, 1);
    assert!(registrations
        .iter()
        .all(|registration| registration.name != "planner" && registration.name != "curator"));
    let pickers = match plan_seats(
        &admitted.resolved,
        &BTreeMap::new(),
        &TargetCatalogue::default(),
        &no_endpoints,
    ) {
        SeatPlan::NeedsChoice(pickers) => pickers,
        SeatPlan::Seats(_) => panic!("a package without models needs a choice"),
    };
    let roles: Vec<_> = pickers.iter().map(|picker| picker.role.as_str()).collect();
    assert_eq!(roles, ["orchestrator"]);
    assert!(!rail(admitted, &[]).starts_planner());
    // No curator registration means no Curator window, so no rounds.
    assert!(registrations
        .iter()
        .all(|registration| !matches!(registration.start, RoleStart::Schedule { .. })));
}

const CUSTOM_WITH_TEAM: &str = "name: dev.example/warden
version: 1.0.0
description: Probe package.
roles:
  orchestrator:
    kind: manager
    instructions: opaque-orchestrator
  security_lead:
    kind: manager
    name: Warden
    start: with_team
    instructions: opaque-security
  auditor:
    kind: manager
    instructions: opaque-auditor
";

#[test]
fn a_custom_with_team_manager_starts_with_the_team_and_an_on_demand_one_is_registered() {
    let admitted = saved(CUSTOM_WITH_TEAM);
    assert_eq!(
        super::seats::started_managers(&admitted.resolved),
        ["orchestrator", "security_lead"]
    );
    let mut picks = BTreeMap::new();
    picks.insert(
        "orchestrator".to_string(),
        OwnerPick::new("probe", Some("m1"), None).unwrap(),
    );
    let SeatPlan::NeedsChoice(pickers) = plan_seats(
        &admitted.resolved,
        &picks,
        &probe_target(&["m1"]),
        &no_endpoints,
    ) else {
        panic!("the custom with_team manager needs its own pick");
    };
    assert_eq!(pickers.len(), 1);
    assert_eq!(
        (pickers[0].role.as_str(), pickers[0].label.as_str()),
        ("security_lead", "Warden")
    );
    picks.insert(
        "security_lead".to_string(),
        OwnerPick::new("probe", Some("m1"), None).unwrap(),
    );
    let SeatPlan::Seats(seats) = plan_seats(
        &admitted.resolved,
        &picks,
        &probe_target(&["m1"]),
        &no_endpoints,
    ) else {
        panic!("every with_team manager is picked");
    };
    assert!(seats.contains_key("security_lead") && !seats.contains_key("auditor"));
    let registrations = registrations_for(TEAM, &admitted, 1);
    let auditor = registrations
        .iter()
        .find(|registration| registration.name == "auditor")
        .expect("an on_demand custom manager is registered");
    assert_eq!(auditor.kind, RoleKind::Manager);
    assert_eq!(auditor.start, RoleStart::Mode(StartMode::OnDemand));
}

#[test]
fn a_schedule_on_a_non_curator_role_is_refused_before_creation() {
    let text = "name: dev.example/scheduled
version: 1.0.0
description: Probe package.
roles:
  orchestrator:
    kind: manager
    instructions: opaque-orchestrator
  auditor:
    kind: manager
    start:
      schedule: 45m..2h
    instructions: opaque-auditor
";
    assert_eq!(
        ResolvedTemplate::from_checked(
            check_yaml(text, &CheckOptions::default()),
            TemplateProvenance::Saved
        )
        .unwrap_err(),
        TemplateStartRefusal::ScheduleUnsupportedRole {
            role: "auditor".into()
        }
    );
}
