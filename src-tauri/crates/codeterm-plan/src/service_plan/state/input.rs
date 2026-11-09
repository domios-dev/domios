use std::borrow::Cow;

use crate::plan::*;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateInput {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub mode: PlanMode,
    pub owner: OrchestratorRef,
    #[serde(default)]
    pub team_id: Option<String>,
    pub project_path: String,
    #[serde(default)]
    pub canonical_project_path: String,
    #[serde(default)]
    pub planner: Option<PlannerRef>,
    #[serde(default)]
    pub planning_state: PlanningState,
    #[serde(default)]
    pub goal: Option<Goal>,
    #[serde(default)]
    pub loops: Vec<LoopSpec>,
    #[serde(default)]
    pub task_groups: Vec<TaskGroup>,
    #[serde(default)]
    pub tag_facets: Vec<TagFacet>,
    #[serde(default)]
    pub spec_refs: Vec<String>,
    #[serde(default)]
    pub spec_links: Vec<SpecLink>,
    #[serde(default)]
    pub primary_spec_id: Option<String>,
    #[serde(default)]
    pub control_loop: ControlLoop,
    #[serde(default)]
    pub review_policy: ReviewPolicy,
    #[serde(default)]
    pub gates: Vec<DecisionGate>,
    #[serde(default)]
    pub regression_gate_overrides: Vec<RegressionGateOverride>,
    #[serde(default)]
    pub orphaned: Option<PlanOrphan>,
    #[serde(default)]
    pub sections: Vec<StateSectionInput>,
    #[serde(default)]
    pub tasks: Vec<StateTaskInput>,
    #[serde(default)]
    pub strides: Vec<Milestone>,
    #[serde(default)]
    pub heartbeat: StateHeartbeatInput,
    #[serde(default)]
    pub undelivered_request_count: usize,
    #[serde(default)]
    pub delivery_uncertain: bool,
    #[serde(default)]
    pub execution_tasks_unlocked: bool,
    #[serde(default)]
    pub orphaned_active: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateTaskInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub review_stage: ReviewStage,
    #[serde(default)]
    pub qa_state: Option<QaState>,
    #[serde(default)]
    pub status_label: Option<String>,
    #[serde(default)]
    pub status_changed_at: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub section_ids: Vec<String>,
    #[serde(default)]
    pub executor_role: Option<String>,
    #[serde(default)]
    pub competition: Option<TaskCompetitionMetadata>,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub assigned_tab_id: Option<String>,
    #[serde(default)]
    pub lane: TaskLaneMetadata,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub review_evidence: Vec<ReviewEvidence>,
    #[serde(default)]
    pub handoff_required: bool,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub execution_attempts: Vec<StateAttemptInput>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateSectionInput {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StateHeartbeatInput {
    pub current_focus: Option<String>,
    #[serde(default)]
    pub active_lanes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateAttemptInput {
    pub id: String,
    #[serde(default)]
    pub executor_tab_id: String,
    #[serde(default)]
    pub closed_at: Option<u64>,
    #[serde(default)]
    pub reported_at: Option<u64>,
    #[serde(default)]
    pub report_target_written_at: Option<u64>,
    #[serde(default)]
    pub parent_tab_id: Option<String>,
    #[serde(default)]
    pub orchestrator_tab_id: Option<String>,
    #[serde(default)]
    pub competition_group_id: Option<String>,
    #[serde(default)]
    pub report_status: Option<String>,
}

pub trait StateSource {
    fn state_input(&self) -> Cow<'_, StateInput>;

    fn mission_control(&self, now_ms: u64) -> super::mission_control::MissionControl {
        super::mission_control::build_mission_control_facts(self.state_input().as_ref(), now_ms)
    }
}

impl StateSource for StateInput {
    fn state_input(&self) -> Cow<'_, StateInput> {
        Cow::Borrowed(self)
    }
}

impl StateSource for Plan {
    fn state_input(&self) -> Cow<'_, StateInput> {
        Cow::Owned(self.into())
    }

    fn mission_control(&self, now_ms: u64) -> super::mission_control::MissionControl {
        super::mission_control::build_mission_control(self, now_ms)
    }
}

pub trait StateTaskSource {
    fn state_task_input(&self) -> Cow<'_, StateTaskInput>;
}

impl StateTaskSource for StateTaskInput {
    fn state_task_input(&self) -> Cow<'_, StateTaskInput> {
        Cow::Borrowed(self)
    }
}

impl StateTaskSource for Task {
    fn state_task_input(&self) -> Cow<'_, StateTaskInput> {
        Cow::Owned(self.into())
    }
}

impl From<&Plan> for StateInput {
    fn from(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            title: plan.title.clone(),
            status: plan.status.clone(),
            mode: plan.mode.clone(),
            owner: plan.owner.clone(),
            team_id: plan.team_id.clone(),
            project_path: plan.project_path.clone(),
            canonical_project_path: plan.canonical_project_path.clone(),
            planner: plan.planner.clone(),
            planning_state: plan.planning_state,
            goal: plan.goal.clone(),
            loops: plan.loops.clone(),
            task_groups: plan.task_groups.clone(),
            tag_facets: plan.tag_facets.clone(),
            spec_refs: plan.spec_refs.clone(),
            spec_links: plan.spec_links.clone(),
            primary_spec_id: plan.primary_spec_id.clone(),
            control_loop: plan.control_loop.clone(),
            review_policy: plan.review_policy.clone(),
            gates: plan.gates.clone(),
            regression_gate_overrides: plan.regression_gate_overrides.clone(),
            orphaned: plan.orphaned.clone(),
            sections: section_inputs(&plan.sections),
            tasks: plan.tasks.iter().map(StateTaskInput::from).collect(),
            strides: plan.strides.clone(),
            heartbeat: StateHeartbeatInput {
                current_focus: plan.heartbeat.current_focus.clone(),
                active_lanes: plan.heartbeat.active_lanes.clone(),
            },
            undelivered_request_count: plan
                .planner_requests
                .iter()
                .filter(|request| request.state != PlannerRequestState::Completed)
                .count(),
            delivery_uncertain: plan
                .planner_inbox
                .iter()
                .any(|entry| entry.phase == PlannerDeliveryPhase::DeliveryUncertain),
            execution_tasks_unlocked: plan.execution_tasks_unlocked(),
            orphaned_active: plan.is_orphaned_active(),
        }
    }
}

fn section_inputs(sections: &[Section]) -> Vec<StateSectionInput> {
    sections
        .iter()
        .flat_map(|section| {
            let mut inputs = vec![StateSectionInput {
                id: section.id.clone(),
                title: section.title.clone(),
                body: section.body.clone(),
            }];
            inputs.extend(section_inputs(&section.subsections));
            inputs
        })
        .collect()
}

impl From<&Task> for StateTaskInput {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            name: task.name.clone(),
            key: task.key.clone(),
            handle: task.handle.clone(),
            status: task.status.clone(),
            review_stage: task.review_stage.clone(),
            qa_state: task.qa_state.clone(),
            status_label: task.status_label.clone(),
            status_changed_at: task.status_changed_at,
            tags: task.tags.clone(),
            section_ids: task.section_ids.clone(),
            executor_role: task.executor_role.clone(),
            competition: task.competition.clone(),
            parent_task_id: task.parent_task_id.clone(),
            depends_on: task.depends_on.clone(),
            assigned_tab_id: task.assigned_tab_id.clone(),
            lane: task.lane.clone(),
            peer: task.peer.clone(),
            group: task.group.clone(),
            review_evidence: task.review_evidence.clone(),
            handoff_required: task.interruption.handoff_required,
            milestones: task.milestones.clone(),
            execution_attempts: task
                .execution_attempts
                .iter()
                .map(|attempt| StateAttemptInput {
                    id: attempt.id.clone(),
                    executor_tab_id: attempt.executor_tab_id.clone(),
                    closed_at: attempt.closed_at,
                    reported_at: attempt.reported_at,
                    report_target_written_at: attempt.report_target_written_at,
                    parent_tab_id: attempt.parent_tab_id.clone(),
                    orchestrator_tab_id: attempt.orchestrator_tab_id.clone(),
                    competition_group_id: attempt.competition_group_id.clone(),
                    report_status: attempt
                        .report
                        .as_ref()
                        .and_then(|report| report.status.clone()),
                })
                .collect(),
        }
    }
}

impl StateTaskInput {
    pub fn display_key(&self) -> String {
        self.key
            .clone()
            .unwrap_or_else(|| crate::plan::task_key::fallback_key(&self.id))
    }
}

impl From<&StateInput> for crate::plan::coverage::PlanCoverageInput {
    fn from(input: &StateInput) -> Self {
        Self {
            sections: input
                .sections
                .iter()
                .map(|section| crate::plan::coverage::CoverageSectionInput {
                    id: section.id.clone(),
                    title: section.title.clone(),
                })
                .collect(),
            tasks: input
                .tasks
                .iter()
                .map(|task| crate::plan::coverage::CoverageTaskInput {
                    name: task.name.clone(),
                    status: task.status.clone(),
                    section_ids: task.section_ids.clone(),
                })
                .collect(),
            spec_links: input.spec_links.clone(),
        }
    }
}

impl<T: StateSource + ?Sized> StateSource for &T {
    fn state_input(&self) -> Cow<'_, StateInput> {
        (*self).state_input()
    }

    fn mission_control(&self, now_ms: u64) -> super::mission_control::MissionControl {
        (*self).mission_control(now_ms)
    }
}

impl<T: StateTaskSource + ?Sized> StateTaskSource for &T {
    fn state_task_input(&self) -> Cow<'_, StateTaskInput> {
        (*self).state_task_input()
    }
}
