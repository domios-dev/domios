//! Spawn gating, boundary arming and the composed role contract. Prompt text
//! is carried as opaque fragments here; wording belongs to the .md files.

use std::fs;

use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::plan::{AgentRef, Milestone, OrchestratorRef, Task, TaskGroup, TaskStatus};

fn map_root(with_sources: bool) -> TempDir {
    let temp = tempfile::tempdir().expect("create cartographer fixture root");
    if with_sources {
        let essences = temp.path().join("essences");
        fs::create_dir_all(&essences).expect("create fixture essences");
        for name in ["capabilities.md", "core.md"] {
            fs::write(essences.join(name), "# fixture\n").expect("write fixture essence");
        }
    }
    temp
}

fn plan() -> Plan {
    let mut plan = Plan::new(
        "Cartographer".into(),
        "/tmp/project".into(),
        OrchestratorRef::from_tab("tab-owner"),
    );
    plan.id = "pl_map".into();
    plan
}

fn task(id: &str, group: Option<&str>, status: TaskStatus, closed_at: u64) -> Task {
    let mut task: Task = serde_json::from_value(json!({
        "id": id,
        "name": id,
        "type": "status_update",
        "model": null,
        "model_rationale": null,
        "test_plan": null,
        "correctness": null,
        "status": status,
        "status_label": null,
        "intent": null,
        "assigned_tab_id": null,
        "peer": null,
    }))
    .expect("build fixture task");
    task.group = group.map(str::to_string);
    task.status_changed_at = closed_at;
    task
}

fn milestone(kind: &str, ts: u64) -> Milestone {
    Milestone {
        id: format!("ms_{kind}_{ts}"),
        from: AgentRef::from_tab("tab-owner"),
        kind: kind.into(),
        text: "boundary".into(),
        ts,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn closed_wave() -> Plan {
    let mut plan = plan();
    plan.task_groups = vec![TaskGroup {
        key: "wave-1".into(),
        label: "First wave".into(),
        color: None,
    }];
    plan.tasks = vec![
        task("task_a", Some("wave-1"), TaskStatus::Done, 100),
        task("task_b", Some("wave-1"), TaskStatus::Done, 200),
        task("task_open", None, TaskStatus::InProgress, 50),
    ];
    plan
}

#[test]
fn cartographer_map_is_unavailable_unless_enabled_and_sourced() {
    let sourced = map_root(true);
    let bare = map_root(false);

    assert_eq!(availability(true, sourced.path()), MapAvailability::Enabled);
    assert_eq!(
        availability(false, sourced.path()),
        MapAvailability::DisabledByConfig
    );
    assert_eq!(
        availability(true, bare.path()),
        MapAvailability::SourcesAbsent
    );
    assert_eq!(
        availability(false, bare.path()),
        MapAvailability::DisabledByConfig
    );
    for state in [
        MapAvailability::DisabledByConfig,
        MapAvailability::SourcesAbsent,
    ] {
        assert!(!state.is_enabled());
        assert_eq!(pending_boundary(&closed_wave(), state), None);
        assert!(boundary_digest_lines(&closed_wave(), state).is_empty());
    }
}

#[test]
fn cartographer_map_arms_once_per_wave_close_and_never_continuously() {
    let plan = closed_wave();
    let enabled = MapAvailability::Enabled;

    assert_eq!(
        pending_boundary(&plan, enabled),
        Some(Boundary::WaveClose {
            key: "wave-1".into(),
            label: "First wave".into(),
        })
    );
    assert_eq!(boundary_digest_lines(&plan, enabled).len(), 1);

    // A dispatched run disarms the offer, and so does a completed one: the
    // next heartbeat over the same boundary yields nothing.
    for kind in [MAP_REQUESTED_KIND, MAP_GENERATED_KIND] {
        let mut dispatched = plan.clone();
        dispatched.strides = vec![milestone(kind, 300)];
        assert_eq!(pending_boundary(&dispatched, enabled), None, "{kind}");
        assert!(
            boundary_digest_lines(&dispatched, enabled).is_empty(),
            "{kind}"
        );
    }

    // A record older than the boundary leaves it armed.
    let mut stale = plan.clone();
    stale.strides = vec![milestone(MAP_GENERATED_KIND, 150)];
    assert!(pending_boundary(&stale, enabled).is_some());

    // A milestone on the Cartographer's own task disarms it the same way.
    let mut on_task = plan.clone();
    on_task.tasks[0].milestones = vec![milestone(MAP_REQUESTED_KIND, 300)];
    assert_eq!(pending_boundary(&on_task, enabled), None);
}

#[test]
fn cartographer_map_offers_one_boundary_at_a_time_and_release_wins() {
    let enabled = MapAvailability::Enabled;
    let mut plan = closed_wave();
    plan.task_groups.push(TaskGroup {
        key: "wave-2".into(),
        label: "Second wave".into(),
        color: None,
    });
    plan.tasks
        .push(task("task_c", Some("wave-2"), TaskStatus::Done, 400));

    // Two closed waves still offer exactly one run, the newest boundary.
    assert_eq!(boundary_digest_lines(&plan, enabled).len(), 1);
    assert_eq!(
        pending_boundary(&plan, enabled),
        Some(Boundary::WaveClose {
            key: "wave-2".into(),
            label: "Second wave".into(),
        })
    );

    // Closing the last open task makes the plan terminal; release dominates.
    plan.tasks[2].status = TaskStatus::Done;
    plan.tasks[2].status_changed_at = 500;
    assert_eq!(pending_boundary(&plan, enabled), Some(Boundary::Release));
    let lines = boundary_digest_lines(&plan, enabled);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with(CARTOGRAPHY_DIGEST_PREFIX));
    assert!(lines[0].contains(&plan.id));
    assert!(lines[0].contains(MAP_REQUESTED_KIND));
}

#[test]
fn cartographer_map_boundary_needs_a_delivered_wave() {
    let enabled = MapAvailability::Enabled;
    let mut plan = closed_wave();
    for task in &mut plan.tasks {
        if task.group.is_some() {
            task.status = TaskStatus::Cancelled;
        }
    }
    assert_eq!(pending_boundary(&plan, enabled), None);

    let mut open = closed_wave();
    open.tasks[1].status = TaskStatus::InProgress;
    assert_eq!(pending_boundary(&open, enabled), None);
}

/// The role's own contract is delivered through the shared preset seam, layered
/// on the Worker base it narrows, and the task-owned persona still leads.
#[test]
fn cartographer_map_role_contract_layers_on_the_worker_base() {
    use codeterm_types::agent::role_kind::builtins;

    use crate::multiplexer::types::AgentRole;
    use crate::service::persona::{self, Occasion};

    const TASK_PERSONA_TOKEN: &str = "OPAQUE-CARTOGRAPHER-PERSONA-4b71d";

    let preset = persona::resolve_ambient(
        Some(AgentRole::Worker),
        Some(builtins::CARTOGRAPHER),
        Some(TASK_PERSONA_TOKEN),
    )
    .expect("the cartographer kind resolves a preset");
    assert_eq!(preset.kind_id, builtins::CARTOGRAPHER);
    assert_eq!(preset.label, "Cartographer");

    let contract = persona::guidance_for(
        Occasion::LaunchAssignment,
        Some(AgentRole::Worker),
        Some(builtins::CARTOGRAPHER),
        Some(TASK_PERSONA_TOKEN),
    );
    for fragment in [
        crate::prompts::WORKER_SUFFIX.trim(),
        crate::prompts::CARTOGRAPHER_SUFFIX.trim(),
    ] {
        assert!(!fragment.is_empty(), "an empty fragment proves nothing");
        assert_eq!(
            contract.matches(fragment).count(),
            1,
            "the cartographer contract carries each fragment exactly once"
        );
    }
    assert!(
        !contract.contains(TASK_PERSONA_TOKEN),
        "the task persona is the identity slot, not the kind contract"
    );

    let identity = persona::guidance_for(
        Occasion::WorkerLaunchIdentity,
        Some(AgentRole::Worker),
        Some(builtins::CARTOGRAPHER),
        Some(TASK_PERSONA_TOKEN),
    );
    assert_eq!(identity.trim(), TASK_PERSONA_TOKEN);
}

/// Measured after the prompt diet, normalized to LF so a CRLF checkout measures
/// the same bytes. The Worker base plus one checklist, and no second manual.
///
/// Re-measured for the project contract that 7244934d6 taught this role while
/// the gate was not being run; it put the surface 47 bytes over the previous
/// 7_600. Raised to the new measurement rather than trimming the contract that
/// was deliberately added.
const CARTOGRAPHER_CONTRACT_MAX_BYTES: usize = 7_650;

#[test]
fn cartographer_map_contract_stays_within_its_measured_ceiling() {
    let rendered = crate::prompts::cartographer_suffix().replace("\r\n", "\n");
    assert!(
        rendered.len() <= CARTOGRAPHER_CONTRACT_MAX_BYTES,
        "the cartographer contract is {} bytes, over its {CARTOGRAPHER_CONTRACT_MAX_BYTES}-byte ceiling",
        rendered.len()
    );
    assert!(rendered.len() <= crate::prompts::COMPOSED_ROLE_PROMPT_MAX_BYTES);
    assert!(
        !slot_residue(&rendered),
        "the contract substitutes no slots, so none may survive"
    );
}

/// Any `{snake_case}` slot left in a composed surface.
fn slot_residue(text: &str) -> bool {
    let mut from = 0;
    while let Some(offset) = text[from..].find('{') {
        let open = from + offset;
        let name = text[open + 1..]
            .bytes()
            .take_while(|byte| byte.is_ascii_lowercase() || *byte == b'_')
            .count();
        if name > 0 && text.as_bytes().get(open + 1 + name) == Some(&b'}') {
            return true;
        }
        from = open + 1;
    }
    false
}

/// The launch a dispatched Cartographer actually receives: the task-owned
/// identity leads, and the kind contract rides behind it in one composition.
#[test]
fn cartographer_map_launch_carries_the_kind_contract_behind_the_task_identity() {
    use codeterm_types::agent::role_kind::builtins;

    use crate::service::plan::worker_identity::{
        attach_worker_identity, compose_worker_identity_contract,
    };
    use crate::service::role_onboarding_task;

    const NAME: &str = "Mercator";
    const GUIDANCE: &str = "OPAQUE-CARTOGRAPHER-GUIDANCE-8f2a1";
    const TASK: &str = "OPAQUE-CARTOGRAPHER-TASK-3d90c";

    let identity = compose_worker_identity_contract(
        Some(NAME),
        Some(GUIDANCE),
        Some(builtins::CARTOGRAPHER),
        TASK,
    );
    let brief = attach_worker_identity(&identity, &format!("# {TASK}\n\nbody\n"));
    let launched = role_onboarding_task(Some(&brief), Some("Worker"), Some(builtins::CARTOGRAPHER))
        .expect("a cartographer launch composes");
    let plain =
        role_onboarding_task(Some(&brief), Some("Worker"), None).expect("a worker launch composes");

    let contract = crate::prompts::CARTOGRAPHER_SUFFIX.trim();
    assert_eq!(launched.matches(contract).count(), 1);
    assert_eq!(
        launched
            .matches(crate::prompts::WORKER_SUFFIX.trim())
            .count(),
        1,
        "the subtype layers on the Worker base rather than replacing it"
    );
    assert!(
        !plain.contains(contract),
        "a plain Worker receives no map contract"
    );

    let guidance_at = launched.find(GUIDANCE).expect("task guidance delivered");
    let contract_at = launched.find(contract).expect("kind contract delivered");
    assert!(guidance_at < contract_at, "task framing leads the launch");
    assert_eq!(launched.matches(GUIDANCE).count(), 1);
}
