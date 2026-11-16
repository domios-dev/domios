//! Delivery, substitution, and size mechanics of the composed agent-facing
//! surfaces. Fragments are opaque tokens here; wording belongs to the .md files.

use crate::prompts::{
    orchestrator_first_message, AGENT_CODETERM_PRIMER, AGENT_PAD_RULE, BRIDGE_MEM, CODETERM_RULE,
    COMPOSED_ROLE_PROMPT_MAX_BYTES, MEM_MANAGEMENT, PLAN_TASK_FLAGS, PLUGIN_DISCOVERY_RULE,
    QA_PIPELINE_DISCIPLINE, RICH_RENDERING_GUIDANCE, TESTING_DISCIPLINE, WORKER_CODETERM_CONTEXT,
};
use crate::service::builtin_roles::builtin_prose;
use crate::service::plan::worker_charter::render_worker_charter;
use crate::service::plan::worker_identity::{
    attach_worker_identity, compose_worker_identity_contract, WORKER_IDENTITY_END,
};
use crate::service::role_onboarding_task;

const WORKER_NAME_TOKEN: &str = "Quillfeather";
const WORKER_ROLE_TOKEN: &str = "opaque-executor-role-5e2d9";
const WORKER_GUIDANCE_TOKEN: &str = "OPAQUE-ROLE-GUIDANCE-71b0c";
const WORKER_TASK_TOKEN: &str = "OPAQUE-TASK-NAME-a93f4";
const WORKER_BODY_TOKEN: &str = "OPAQUE-TASK-BODY-2c8e1";
const PLANNER_ASSIGNMENT_TOKEN: &str = "OPAQUE-PLANNER-ASSIGNMENT-d41a7";
const CHARTER_TASK_TOKEN: &str = "task_opaque_6f3b2";

// Primer and worker launch re-measured for the owner-requested Sentinel command line
// (bench case 9 needs the concrete flags) and the `ds-guide` rendering sentence.
const PRIMER_MAX_BYTES: usize = 2_600;
const PROVIDER_RULES_MAX_BYTES: usize = 9_700;
const WORKER_CHARTER_MAX_BYTES: usize = 1_450;
const WORKER_LAUNCH_OVERHEAD_MAX_BYTES: usize = 9_000;

fn lf_len(text: &str) -> usize {
    text.replace("\r\n", "\n").len()
}

fn orchestrator_launch() -> String {
    role_onboarding_task(None, Some("Orchestrator"), None, None).expect("orchestrator launch")
}

fn planner_launch() -> String {
    role_onboarding_task(
        Some(PLANNER_ASSIGNMENT_TOKEN),
        Some("Manager"),
        Some("planner"),
        None,
    )
    .expect("planner launch")
}

fn worker_brief() -> (String, String) {
    let identity = compose_worker_identity_contract(
        Some(WORKER_NAME_TOKEN),
        Some(WORKER_GUIDANCE_TOKEN),
        Some(WORKER_ROLE_TOKEN),
        WORKER_TASK_TOKEN,
    );
    let brief = attach_worker_identity(
        &identity,
        &format!("# {WORKER_TASK_TOKEN}\n\n{WORKER_BODY_TOKEN}\n"),
    );
    (identity, brief)
}

fn worker_launch(brief: &str) -> String {
    role_onboarding_task(Some(brief), Some("Worker"), None, None).expect("worker launch")
}

fn charter() -> String {
    render_worker_charter(CHARTER_TASK_TOKEN, Some(WORKER_GUIDANCE_TOKEN), None, 1, 0)
}

/// Every `{snake_case}` slot left in a composed surface.
fn slot_residue(text: &str) -> Vec<String> {
    let mut residue = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find('{') {
        let open = from + offset;
        let name_len = text[open + 1..]
            .bytes()
            .take_while(|byte| byte.is_ascii_lowercase() || *byte == b'_')
            .count();
        if name_len > 0 && text.as_bytes().get(open + 1 + name_len) == Some(&b'}') {
            residue.push(text[open..open + name_len + 2].to_string());
        }
        from = open + 1;
    }
    residue
}

/// Every builtin slot and worker type, as one launch composes it with an opaque assignment.
fn composed_builtin_launches() -> Vec<(&'static str, String)> {
    crate::service::role_preset::builtin_presets()
        .into_iter()
        .map(|(kind, preset)| {
            let role = preset.role.label();
            let launch =
                role_onboarding_task(Some(PLANNER_ASSIGNMENT_TOKEN), Some(role), Some(kind), None)
                    .expect("builtin launch");
            (kind, launch)
        })
        .collect()
}

#[test]
fn prompt_diet_every_composed_slot_and_worker_stays_within_the_role_budget() {
    let measured: Vec<(&str, usize)> = composed_builtin_launches()
        .iter()
        .map(|(kind, launch)| (*kind, lf_len(launch)))
        .collect();
    let over: Vec<_> = measured
        .iter()
        .filter(|(_, bytes)| *bytes > COMPOSED_ROLE_PROMPT_MAX_BYTES)
        .collect();
    assert!(
        over.is_empty(),
        "composed launches over {COMPOSED_ROLE_PROMPT_MAX_BYTES} B: {over:?}; all: {measured:?}"
    );
}

#[test]
fn prompt_diet_shared_surfaces_stay_within_measured_ceilings() {
    let (_, brief) = worker_brief();
    let worker = worker_launch(&brief);
    let worker_overhead = lf_len(&worker).saturating_sub(lf_len(&brief));

    for (name, measured, ceiling) in [
        (
            "spawned-agent primer",
            lf_len(AGENT_CODETERM_PRIMER),
            PRIMER_MAX_BYTES,
        ),
        (
            "provider rules",
            lf_len(CODETERM_RULE) + lf_len(BRIDGE_MEM),
            PROVIDER_RULES_MAX_BYTES,
        ),
        (
            "worker charter",
            lf_len(&charter()),
            WORKER_CHARTER_MAX_BYTES,
        ),
        (
            "worker launch overhead",
            worker_overhead,
            WORKER_LAUNCH_OVERHEAD_MAX_BYTES,
        ),
    ] {
        assert!(
            measured <= ceiling,
            "{name} is {measured} bytes, over its {ceiling}-byte ceiling"
        );
    }
}

/// The task-owned identity leads the Worker's launch: every spawn input arrives
/// exactly once and ahead of each generic fragment, and the transport marker
/// leaves no residue.
#[test]
fn prompt_diet_worker_launch_leads_with_task_identity() {
    let (identity, brief) = worker_brief();
    let composed = worker_launch(&brief);

    let first_generic = [
        AGENT_CODETERM_PRIMER,
        WORKER_CODETERM_CONTEXT,
        AGENT_PAD_RULE,
        RICH_RENDERING_GUIDANCE,
        builtin_prose("worker").expect("worker prose"),
    ]
    .iter()
    .map(|fragment| {
        composed
            .find(fragment.trim())
            .expect("the worker launch carries every generic fragment")
    })
    .min()
    .expect("generic fragments exist");

    for token in [WORKER_NAME_TOKEN, WORKER_ROLE_TOKEN, WORKER_GUIDANCE_TOKEN] {
        assert!(identity.contains(token), "the identity carries {token}");
        assert_eq!(composed.matches(token).count(), 1, "{token} arrives once");
        assert!(
            composed.find(token).expect("present") < first_generic,
            "{token} must precede every generic fragment"
        );
    }
    assert!(composed.find(WORKER_TASK_TOKEN).expect("task named") < first_generic);
    assert_eq!(composed.matches(WORKER_BODY_TOKEN).count(), 1);
    assert!(!composed.contains(WORKER_IDENTITY_END));
}

#[test]
fn prompt_diet_role_launches_receive_each_fragment_once() {
    let (_, brief) = worker_brief();
    let shared = [
        AGENT_CODETERM_PRIMER,
        PLUGIN_DISCOVERY_RULE,
        AGENT_PAD_RULE,
        RICH_RENDERING_GUIDANCE,
    ];

    let orchestrator_fragments = [
        builtin_prose("orchestrator").expect("orchestrator prose"),
        PLAN_TASK_FLAGS,
        MEM_MANAGEMENT,
    ];
    let planner_fragments = [
        builtin_prose("planner").expect("planner prose"),
        PLAN_TASK_FLAGS,
        MEM_MANAGEMENT,
    ];
    let worker_fragments = [
        builtin_prose("worker").expect("worker prose"),
        WORKER_CODETERM_CONTEXT,
        TESTING_DISCIPLINE,
        QA_PIPELINE_DISCIPLINE,
    ];

    for (recipient, composed, fragments) in [
        (
            "orchestrator",
            orchestrator_launch(),
            &orchestrator_fragments[..],
        ),
        ("planner", planner_launch(), &planner_fragments[..]),
        ("worker", worker_launch(&brief), &worker_fragments[..]),
    ] {
        for fragment in shared.iter().chain(fragments) {
            let token = fragment.trim();
            assert!(!token.is_empty(), "an empty fragment proves nothing");
            assert_eq!(
                composed.matches(token).count(),
                1,
                "{recipient} must receive each of its fragments exactly once"
            );
        }
    }
}

#[test]
fn prompt_diet_composed_surfaces_carry_no_slot_residue() {
    let (_, brief) = worker_brief();
    let mut surfaces: Vec<(&str, String)> = composed_builtin_launches();
    surfaces.extend([
        ("orchestrator launch", orchestrator_launch()),
        ("planner launch", planner_launch()),
        ("worker launch", worker_launch(&brief)),
        ("worker charter", charter()),
        (
            "orchestrator first message",
            orchestrator_first_message("tab-x", "team-y", Some("pl_z")),
        ),
        ("codeterm rule", CODETERM_RULE.to_string()),
        ("bridge mem", BRIDGE_MEM.to_string()),
    ]);
    for (name, composed) in surfaces {
        assert_eq!(
            slot_residue(&composed),
            Vec::<String>::new(),
            "{name} still carries an unsubstituted slot"
        );
    }
    assert_eq!(slot_residue("keep {plan_id} here"), vec!["{plan_id}"]);
}

/// The charter reinjection keeps the supplied identity ahead of the shared
/// verification discipline it restates.
#[test]
fn prompt_diet_worker_charter_carries_identity_before_discipline() {
    let rendered = charter();
    assert_eq!(rendered.matches(CHARTER_TASK_TOKEN).count(), 2);
    assert_eq!(rendered.matches(WORKER_GUIDANCE_TOKEN).count(), 1);
    let identity = rendered.find(WORKER_GUIDANCE_TOKEN).expect("identity");
    for fragment in [TESTING_DISCIPLINE, QA_PIPELINE_DISCIPLINE] {
        let position = rendered
            .find(fragment.trim())
            .expect("discipline delivered");
        assert!(identity < position);
    }
}

#[test]
fn memory_management_fragment_reaches_only_target_launches() {
    let fragment = MEM_MANAGEMENT.trim();
    assert!(!fragment.is_empty());
    for (role, kind, expected) in [
        (Some("Orchestrator"), None, 1),
        (Some("Manager"), Some("planner"), 1),
        (None, None, 1),
        (Some("Worker"), None, 0),
        (Some("Manager"), Some("cartographer"), 0),
    ] {
        for assignment in [None, Some("OPAQUE_MEMORY_ASSIGNMENT")] {
            let rendered =
                role_onboarding_task(assignment, role, kind, None).expect("launch prompt");
            assert_eq!(
                rendered.matches(fragment).count(),
                if kind == Some("planner") && assignment.is_none() {
                    0
                } else {
                    expected
                },
                "{role:?}/{kind:?}"
            );
            assert!(slot_residue(&rendered).is_empty());
            if let Some(token) = assignment {
                assert_eq!(rendered.matches(token).count(), 1);
            }
        }
    }
    for rendered in [
        crate::service::role_preset::builtin_guidance("worker").expect("worker"),
        crate::service::role_preset::builtin_guidance("cartographer").expect("cartographer"),
        crate::prompts::CRON_COMPLETE_SUFFIX.to_string(),
        crate::prompts::TOOL_LESS.to_string(),
    ] {
        assert_eq!(rendered.matches(fragment).count(), 0);
    }
}
