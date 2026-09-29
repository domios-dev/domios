//! Brief-composition and persona tests (split from tests.rs for the size gate).
use super::*;

#[test]
fn dispatched_worker_launch_prompt_carries_ack_instruction_persona_and_brief() {
    let task_id_token = "task_dispatched_brief_roundtrip_82913";
    let personality_token = "persona_dispatched_brief_roundtrip_03174";
    let body_token = "body_dispatched_brief_roundtrip_75820";
    let t = task(json!({
        "id": task_id_token,
        "name": task_id_token,
        "type": "code",
        "status": "pending",
        "personality": personality_token,
        "personality_provenance": IdentityProvenance::UserSupplied,
        "body": body_token,
    }));
    let brief = super::super::canonical_writer::compose_dispatch_brief(
        build_brief_with_manifest(&t, &[], "plan_dispatched_brief", &[], &[]),
        crate::plan::WorkerPlacement::IsolatedLane,
    );
    let mut spawn_params = crate::service::SpawnAgentParams {
        role: Some("Worker".into()),
        plan_id: Some("plan_dispatched_brief".into()),
        task_id: Some(task_id_token.into()),
        ..Default::default()
    };

    set_dispatch_task_brief(&mut spawn_params, brief);

    let launch_prompt = spawn_params
        .task
        .as_deref()
        .expect("dispatch launch carries a task brief");
    assert!(launch_prompt.contains(task_id_token));
    assert!(launch_prompt.contains(personality_token));
    assert!(launch_prompt.contains(body_token));
    assert!(launch_prompt.contains(crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION));
}

#[test]
fn brief_opens_with_personality_line_when_set() {
    let t = task(json!({
        "id": "task_p",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with("You are Hopper — executor for: Wire dispatch RPC"));
    assert!(brief.contains("# Wire dispatch RPC"));
}

#[test]
fn role_guidance_contract_delivery_reaches_the_composed_worker_context_first() {
    let guidance_token = "rg_7d14a091_conditioning";
    let body_token = "body_4a112f";
    let criterion_token = "criterion_c29e03";
    let test_plan_token = "test_plan_91be77";
    let t = task(json!({
        "id": "task_rg",
        "name": "Delivery fixture",
        "type": "code",
        "status": "pending",
        "role_guidance": guidance_token,
        "body": body_token,
        "acceptance_criteria": [criterion_token],
        "test_plan": test_plan_token,
    }));
    let brief = build_brief(&t, &[]);
    let params = crate::service::SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some(brief),
        ..Default::default()
    };
    let context =
        crate::service::spawn_onboarding_task(&params).expect("worker launch context is composed");

    let guidance_pos = context
        .find(guidance_token)
        .expect("supplied guidance arrives");
    let body_pos = context.find(body_token).expect("supplied body arrives");
    let criterion_pos = context
        .find(criterion_token)
        .expect("supplied criterion arrives");
    let test_plan_pos = context
        .find(test_plan_token)
        .expect("supplied test plan arrives");
    assert_eq!(guidance_pos, 0);
    assert!(guidance_pos < body_pos);
    assert!(guidance_pos < criterion_pos);
    assert!(guidance_pos < test_plan_pos);
    assert_eq!(context.matches(guidance_token).count(), 1);
}

#[test]
fn declared_gate_tokens_reach_worker_context_without_placeholder_residue() {
    let gates = vec![
        "gate-token-alpha".to_string(),
        "gate-token-beta".to_string(),
    ];
    let t = task(json!({
        "id": "task_gates",
        "name": "Gate delivery fixture",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief_with_manifest(&t, &[], "plan_gates", &[], &gates);
    let params = crate::service::SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some(brief),
        ..Default::default()
    };
    let context =
        crate::service::spawn_onboarding_task(&params).expect("worker launch context is composed");

    for gate in &gates {
        assert_eq!(context.matches(gate).count(), 1);
    }
    assert!(!context.contains("{authorized_gates}"));
}

#[test]
fn brief_deduplicates_matching_persona_guidance_prefix() {
    let t = task(json!({
        "id": "task_dup",
        "name": "Trace composition",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
        "role_guidance": "You are Hopper. Keep the role guidance first.",
    }));

    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with(
        "You are Hopper. Keep the role guidance first.\n\
         Your assigned task is: Trace composition"
    ));
    assert!(brief.contains("# Trace composition"));
    assert!(!brief.contains("You are Hopper. You are Hopper."));
}

#[test]
fn brief_uses_claimed_personality_once_without_mutating_routing_metadata() {
    let t = task(json!({
        "id": "task_claimed",
        "name": "Trace composition",
        "type": "code",
        "status": "pending",
        "personality": "Suggested Hopper",
        "executor_role": "rust-backend-dev",
        "role_guidance": "You are Grace. Keep the role guidance first.",
    }));

    let brief = build_brief_with_personality(&t, &[], Some("Grace"));
    assert!(brief.starts_with(
        "You are Grace. Keep the role guidance first.\n\
         Executor role: rust-backend-dev.\n\
         Your assigned task is: Trace composition"
    ));
    assert_eq!(brief.matches("You are Grace.").count(), 1);
    assert!(!brief.contains("Suggested Hopper"));
    assert!(brief.contains("Executor role: rust-backend-dev."));
}

#[test]
fn test_author_brief_carries_charter_reference_before_task() {
    let t = task(json!({
        "id": "task_test_author",
        "name": "Author spawn-readiness drill",
        "type": "test",
        "status": "pending",
        "executor_role": "test-author",
        "role_guidance": crate::prompts::TEST_AUTHOR_GUIDANCE,
        "body": "Write the drill that catches a readiness race.",
    }));
    let brief = build_brief(&t, &[]);
    let guidance_pos = brief
        .find(crate::prompts::TEST_AUTHOR_GUIDANCE)
        .expect("canonical test-author guidance in brief");
    let title_pos = brief
        .find("# Author spawn-readiness drill")
        .expect("title in brief");
    let body_pos = brief
        .find("Write the drill that catches a readiness race.")
        .expect("body in brief");

    assert_eq!(guidance_pos, 0);
    assert!(guidance_pos < title_pos);
    assert!(guidance_pos < body_pos);
}

#[test]
fn brief_supplies_task_owned_identity_when_personality_is_absent() {
    let t = task(json!({
        "id": "task_np",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with("You are the Worker responsible for: Bare"));
    assert!(brief.contains("# Bare"));
}

#[test]
fn tab_name_contract_dispatch_preserves_only_explicit_user_personality() {
    let t = task(json!({
        "id": "task_user_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "personality_provenance": IdentityProvenance::UserSupplied,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::UserSupplied));
}

#[test]
fn tab_name_contract_dispatch_treats_planner_personality_as_provisional() {
    let t = task(json!({
        "id": "task_suggested_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "executor_role": "frontend-dev",
        "personality_provenance": IdentityProvenance::Suggested,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::UserSupplied));
}

#[test]
fn tab_name_contract_dispatch_never_uses_executor_role_as_tab_name() {
    let t = task(json!({
        "id": "task_executor_title",
        "name": "Keep identity stable",
        "type": "code",
        "status": "pending",
        "executor_role": "dispatch-identity-dev",
    }));

    let (title, provenance) = dispatch_worker_identity(&t);

    assert_eq!(title, None);
    assert_eq!(provenance, None);
}

#[test]
fn tab_name_contract_dispatch_treats_batch_personality_without_legacy_provenance_as_provisional() {
    let t = task(json!({
        "id": "task_batch_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Lamport",
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Lamport"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::UserSupplied));
}

#[test]
fn socket_dispatch_allows_review_debt_without_capacity_grants() {
    let mut plan = Plan::new(
        "review debt".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
    let review = task(json!({
        "id": "review",
        "name": "review",
        "type": "implement",
        "status": "pending",
        "tags": ["review-capacity"],
    }));
    let implementation = task(json!({
        "id": "implementation",
        "name": "implementation",
        "type": "implement",
        "status": "pending",
    }));

    preflight_review_capacity(&plan, &review).unwrap();
    plan.review_policy
        .capacity_grants
        .push(crate::plan::ReviewCapacityGrant {
            task_id: review.id.clone(),
            class: crate::plan::ReviewCapacityClass::Review,
            granted_by: crate::plan::AgentRef::from_tab("orch"),
            granted_at: 1,
        });
    preflight_review_capacity(&plan, &review).unwrap();
    preflight_review_capacity(&plan, &implementation).unwrap();
}

#[test]
fn brief_includes_body_acceptance_and_task_authority() {
    let t = task(json!({
        "id": "task_1",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "body": "Compose spawn, assign, send.",
        "acceptance_criteria": ["atomic", "returns Tab id"],
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.contains("# Wire dispatch RPC"));
    assert!(brief.contains("Compose spawn, assign, send."));
    assert!(brief.contains("- atomic"));
    assert!(brief.contains("- returns Tab id"));
    assert!(brief.starts_with("You are the Worker responsible for: Wire dispatch RPC"));
    assert!(!brief.contains("codeterm agent report"));
    assert!(!brief.contains("--base "));
}

#[test]
fn worker_prompt_places_ack_before_assignment_work() {
    let persona_token = "worker_persona_roundtrip_60124";
    let guidance_token = "worker_guidance_roundtrip_39517";
    let body_token = "worker_body_roundtrip_28406";
    let intent_token = "worker_intent_roundtrip_94731";
    let correctness_token = "worker_correctness_roundtrip_58320";
    let test_plan_token = "worker_test_plan_roundtrip_17386";
    let t = task(json!({
        "id": "task_prompt_order",
        "name": "Repair prompt order",
        "type": "code",
        "status": "pending",
        "personality": persona_token,
        "executor_role": "rust-backend-dev",
        "role_guidance": guidance_token,
        "body": body_token,
        "intent": intent_token,
        "acceptance_criteria": ["worker_criterion_roundtrip_84260"],
        "correctness": correctness_token,
        "test_plan": test_plan_token
    }));
    let brief = build_brief(&t, &[]);
    let params = crate::service::SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some(brief),
        ..Default::default()
    };
    let prompt = crate::service::spawn_onboarding_task(&params).unwrap();

    for token in [
        persona_token,
        guidance_token,
        body_token,
        intent_token,
        "worker_criterion_roundtrip_84260",
        correctness_token,
        test_plan_token,
    ] {
        assert_eq!(prompt.matches(token).count(), 1);
    }
    let ack_instruction = crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION;
    assert_eq!(prompt.matches(ack_instruction).count(), 1);
    let ack_position = prompt.find(ack_instruction).unwrap();
    let body_position = prompt.find(body_token).unwrap();
    assert!(ack_position < body_position);
    assert!(!prompt.contains(crate::service::plan::worker_identity::WORKER_IDENTITY_END));
}

#[test]
fn brief_lists_section_links_and_test_plan() {
    let t = task(json!({
        "id": "task_2",
        "name": "T",
        "type": "code",
        "status": "pending",
        "test_plan": "cargo test dispatch",
        "qa_plan": "cargo test independent acceptance",
    }));
    let sections = vec![TaskSection {
        id: "sec_9".into(),
        title: "Phase 1".into(),
    }];
    let brief = build_brief(&t, &sections);
    assert!(brief.contains("## Test plan"));
    assert!(brief.contains("cargo test dispatch"));
    assert!(!brief.contains("independent acceptance"));
    assert!(!brief.contains("qa_plan"));
    assert!(brief.contains("## Sections"));
    assert!(brief.contains("Phase 1 (sec_9)"));
}
#[test]
fn brief_omits_empty_optional_sections() {
    let t = task(json!({
        "id": "task_3",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(!brief.contains("## Acceptance criteria"));
    assert!(!brief.contains("## Test plan"));
    assert!(!brief.contains("## Sections"));
    assert!(!brief.contains("## Strides"));
    assert!(!brief.contains("codeterm agent report"));
}

#[test]
fn remote_worker_brief_replaces_the_local_ack_with_the_origin_remote_ack() {
    let ack = crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION;
    let origin = "origin_token_51730";
    let brief = crate::socket::server::plan_task_dispatch::remote::remote_worker_brief(
        format!("{ack}\n\nbody_token_22019"),
        "plan_token",
        "task_token",
        "attempt_token",
        origin,
    );
    assert!(!brief.contains(ack));
    assert!(brief.contains("body_token_22019"));
    assert_eq!(brief.matches(origin).count(), 2);
    assert!(brief.find(origin).unwrap() < brief.find("body_token_22019").unwrap());
}
