use codeterm_blueprint_types::blueprint_contract::{FieldCheck, PipelinePreview};
use codeterm_blueprint_types::blueprint_pipeline::{
    BlueprintOwner, BlueprintSpec, ConverterEdge, ResultTarget, StepField, StepKind, Trigger,
    ValidationError, ValidationPath,
};
use serde_json::{json, Value};

use crate::scheduler::Handle;
use codeterm_service_core::service::ServiceContext;

pub trait BlueprintExt {
    fn blueprint_log(
        &self,
        caller: &crate::service_adapter::SentinelCaller,
        id: &str,
        limit: u16,
    ) -> Result<Vec<codeterm_service_core::storage::BlueprintJournalEntry>, String>;
    fn blueprint_pipeline_validate(
        &self,
        caller: &crate::service_adapter::SentinelCaller,
        spec: BlueprintSpec,
    ) -> Result<PipelinePreview, String>;

    fn blueprint_pipeline_nodes(&self) -> Value;
    fn validate_pipeline(&self, spec: &BlueprintSpec) -> Result<(), String>;
    fn validate_event_trigger(&self, spec: &BlueprintSpec) -> Result<(), String>;
    fn pipeline_validation_errors(
        &self,
        spec: &BlueprintSpec,
    ) -> Result<Vec<ValidationError>, String>;
}

impl BlueprintExt for ServiceContext {
    fn blueprint_log(
        &self,
        caller: &crate::service_adapter::SentinelCaller,
        id: &str,
        limit: u16,
    ) -> Result<Vec<codeterm_service_core::storage::BlueprintJournalEntry>, String> {
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let completion = {
            let store = handle.store.lock();
            store.ensure_available()?;
            if !store.get(id).is_some_and(|job| caller.owns(job)) {
                return Err(format!(
                    "blueprint not found in this workspace and Team: {id}"
                ));
            }
            store.typed_pipeline_log(id, limit)?
        };
        completion
            .wait(crate::storage::COMMIT_DEADLINE)
            .map(|read| read.value)
            .map_err(|error| error.to_string())
    }

    fn blueprint_pipeline_validate(
        &self,
        caller: &crate::service_adapter::SentinelCaller,
        spec: BlueprintSpec,
    ) -> Result<PipelinePreview, String> {
        let errors = self.pipeline_validation_errors(&spec)?;
        let trigger_available = match &spec.trigger {
            Trigger::Event { kind, .. } => self
                .ext::<std::sync::Arc<codeterm_plugin::event_source::EventSourceRegistry>>()
                .select_provider(kind)
                .is_some(),
            trigger => !codeterm_blueprint_types::blueprint_pipeline::trigger_unavailable(trigger),
        };
        let mut checks = vec![
            field_check("name", &errors),
            field_check("trigger", &errors),
        ];
        checks.push(field_check("trigger.params", &errors));
        checks.push(FieldCheck::new(
            "trigger.params",
            self.validate_event_trigger(&spec),
        ));
        if !trigger_available {
            checks[1] = FieldCheck::new(
                "trigger",
                Err(if matches!(&spec.trigger, Trigger::Event { .. }) {
                    "event_source_unavailable"
                } else {
                    "trigger_unavailable"
                }
                .into()),
            );
        }
        for (index, step) in spec.steps.iter().enumerate() {
            checks.push(field_check(&format!("steps[{index}].name"), &errors));
            checks.push(field_check(&format!("steps[{index}].input"), &errors));
            checks.push(field_check(&format!("steps[{index}].on"), &errors));
            checks.push(field_check(&format!("steps[{index}]"), &errors));
            match &step.kind {
                StepKind::Ask { .. } | StepKind::Filter { .. } => {
                    checks.push(field_check(&format!("steps[{index}].buckets"), &errors));
                }
                StepKind::Convert { .. } => {
                    checks.push(field_check(&format!("steps[{index}].convert"), &errors));
                }
                StepKind::Agent { .. } => {
                    checks.push(field_check(&format!("steps[{index}].agent"), &errors));
                }
                StepKind::Http { .. } => {
                    checks.push(field_check(&format!("steps[{index}].http"), &errors));
                }
                StepKind::Github { .. } => {
                    checks.push(field_check(&format!("steps[{index}].github"), &errors));
                }
                StepKind::Command { .. }
                | StepKind::Code { .. }
                | StepKind::Call { .. }
                | StepKind::Effect { .. } => {}
            }
        }
        checks.push(field_check("result.to", &errors));
        let owner_available = spec.owner != BlueprintOwner::Team || caller.team.is_some();
        checks.push(FieldCheck::new(
            "owner",
            if owner_available {
                Ok(())
            } else {
                Err("Team owner requires the creating tab to belong to a Team".into())
            },
        ));
        let (caller_live, result_scope) = {
            let mux = self.mux.read();
            let live = crate::service_adapter::caller_is_live(&mux, caller);
            let target = if live {
                validate_pipeline_result_scope(&mux, caller, spec.owner, &spec.result.to)
            } else {
                Err("blueprint caller scope changed".into())
            };
            (live, target)
        };
        checks.push(FieldCheck::new(
            "scope",
            if caller_live {
                Ok(())
            } else {
                Err("blueprint caller scope changed".into())
            },
        ));
        checks.push(FieldCheck::new("result.scope", result_scope));
        Ok(PipelinePreview {
            chain: chain_summary(&spec),
            normalized_spec: spec,
            checks,
            id: None,
            expect_version: None,
        })
    }

    fn blueprint_pipeline_nodes(&self) -> Value {
        let mut nodes = Trigger::DESCRIPTORS
            .iter()
            .chain(StepKind::DESCRIPTORS)
            .chain(crate::machine::Route::DESCRIPTORS)
            .chain(codeterm_blueprint_types::blueprint_catalog::SOURCES_AND_MODES)
            .map(codeterm_blueprint_types::blueprint_catalog::NodeDescriptor::value)
            .collect::<Vec<_>>();
        for (_, conversion) in self
            .ext::<std::sync::Arc<codeterm_plugin::converter::ConverterRegistry>>()
            .available()
        {
            nodes.push(json!({
                "role":"step",
                "kind":"converter",
                "from":conversion.from.as_str(),
                "to":conversion.to.as_str(),
                "title":format!("{} to {}", conversion.from.as_str(), conversion.to.as_str()),
                "params_schema":{"from":conversion.from.as_str(), "to":conversion.to.as_str()}
            }));
        }
        for provider in self
            .chatbackend_registry
            .active()
            .into_iter()
            .filter(|p| p.supports_agent_tasks())
        {
            nodes.push(json!({"role":"agent", "kind":"agent_provider", "type":provider.id(), "title":provider.display_name(), "profiles":provider.list_presets().unwrap_or_default()}));
        }
        let event_sources = self
            .ext::<std::sync::Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>()
            .event_sources();
        for source in &event_sources {
            let schemas = source.to_value();
            nodes.push(json!({
                "role":"trigger",
                "kind":"event_source",
                "type":source.kind,
                "title":source.title,
                "params_schema":schemas.get("params").cloned().unwrap_or(Value::Null),
                "payload_schema":schemas.get("payload").cloned().unwrap_or(Value::Null)
            }));
        }
        Value::Array(nodes)
    }

    fn validate_pipeline(&self, spec: &BlueprintSpec) -> Result<(), String> {
        let errors = self.pipeline_validation_errors(spec)?;
        if errors.is_empty() {
            return Ok(());
        }
        Err(errors
            .into_iter()
            .map(|error| format!("{}: {}", error.field, error.reason))
            .collect::<Vec<_>>()
            .join("; "))
    }

    fn validate_event_trigger(&self, spec: &BlueprintSpec) -> Result<(), String> {
        let Trigger::Event { kind, params } = &spec.trigger else {
            return Ok(());
        };
        let encoded = serde_json::to_vec(params).map_err(|error| error.to_string())?;
        if encoded.len() > codeterm_plugin::graph_events::MAX_EVENT_BYTES {
            return Err("trigger.params: exceeds 64 KiB".into());
        }
        if let Some(provider) = self
            .ext::<std::sync::Arc<codeterm_plugin::event_source::EventSourceRegistry>>()
            .select_provider(kind)
        {
            if let Some(schema) = provider.params_schema(kind) {
                schema
                    .validate_instance(params)
                    .map_err(|error| format!("trigger.params: {error}"))?;
            }
        }
        Ok(())
    }

    fn pipeline_validation_errors(
        &self,
        spec: &BlueprintSpec,
    ) -> Result<Vec<ValidationError>, String> {
        let definition_bytes = serde_json::to_vec(spec).map_err(|error| error.to_string())?;
        if definition_bytes.len() > 65_536 {
            return Ok(vec![ValidationError::new(
                "spec",
                "blueprint definition exceeds 64 KiB",
            )]);
        }
        let converters = self
            .ext::<std::sync::Arc<codeterm_plugin::converter::ConverterRegistry>>()
            .available()
            .into_iter()
            .map(|(_, edge)| ConverterEdge {
                from: edge.from,
                to: edge.to,
            })
            .collect::<Vec<_>>();
        let mut errors = codeterm_blueprint_types::blueprint_pipeline::validate(spec, &converters)
            .err()
            .unwrap_or_default();
        for (index, step) in spec.steps.iter().enumerate() {
            if let StepKind::Agent { provider, .. } = &step.kind {
                if !self
                    .chatbackend_registry
                    .get(provider)
                    .is_some_and(|p| p.supports_agent_tasks())
                {
                    errors.push(ValidationError::new(
                        ValidationPath::Step {
                            index,
                            field: StepField::Parameters("agent".into()),
                        },
                        "provider does not support bounded agent tasks",
                    ));
                }
            }
            if let StepKind::Convert { from, to } = &step.kind {
                let field = format!("steps[{index}].convert");
                if self
                    .ext::<std::sync::Arc<codeterm_plugin::converter::ConverterRegistry>>()
                    .plan(*from, *to)
                    .is_err()
                    && !errors.iter().any(|error| error.field == field)
                {
                    errors.push(ValidationError::new(
                        ValidationPath::Step {
                            index,
                            field: StepField::Parameters("convert".into()),
                        },
                        "no converter chain is available",
                    ));
                }
            }
        }
        Ok(errors)
    }
}

pub fn field_check(field: &str, errors: &[ValidationError]) -> FieldCheck {
    let failures = errors
        .iter()
        .filter(|error| {
            error.field == field
                || error
                    .field
                    .strip_prefix(field)
                    .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
        })
        .map(|error| format!("{}: {}", error.field, error.reason))
        .collect::<Vec<_>>();
    FieldCheck::new(
        field,
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        },
    )
}

fn validate_pipeline_result_scope(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    caller: &crate::service_adapter::SentinelCaller,
    owner: BlueprintOwner,
    target: &ResultTarget,
) -> Result<(), String> {
    match target {
        ResultTarget::Owner if owner == BlueprintOwner::Team => {
            team_result_recipient(mux, caller.team.as_deref(), &caller.workspace)?;
            Ok(())
        }
        ResultTarget::Owner | ResultTarget::Notify => Ok(()),
        ResultTarget::Team if caller.team.is_none() => {
            Err("result.to: team delivery requires a Team owner".into())
        }
        ResultTarget::Team => {
            team_result_recipient(mux, caller.team.as_deref(), &caller.workspace)?;
            Ok(())
        }
        ResultTarget::Tab(id) => {
            let target = mux.resolve_tab_id(id).ok_or("result.to: tab not found")?;
            let tab = mux.get_tab(&target).ok_or("result.to: tab not found")?;
            if tab.team_id != caller.team
                || mux.find_workspace_for_tab(&target).as_deref() != Some(&caller.workspace)
                || tab.agent_type.is_none()
            {
                return Err(
                    "result.to: tab must be an agent in the creator's Team and workspace".into(),
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn validate_result_scope(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    job: &crate::store::Sentinel,
    target: &ResultTarget,
) -> Result<(), String> {
    match target {
        ResultTarget::Owner | ResultTarget::Notify => Ok(()),
        ResultTarget::Team if job.team_id.is_none() => {
            Err("result.to: team delivery requires a Team owner".into())
        }
        ResultTarget::Team => {
            team_result_recipient(mux, job.team_id.as_deref(), &job.workspace_id)?;
            Ok(())
        }
        ResultTarget::Tab(id) => {
            let target = mux.resolve_tab_id(id).ok_or("result.to: tab not found")?;
            let tab = mux.get_tab(&target).ok_or("result.to: tab not found")?;
            if tab.team_id != job.team_id
                || mux.find_workspace_for_tab(&target).as_deref() != Some(&job.workspace_id)
                || tab.agent_type.is_none()
            {
                return Err(
                    "result.to: tab must be an agent in the creator's Team and workspace".into(),
                );
            }
            Ok(())
        }
    }
}

fn team_result_recipient(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    team_id: Option<&str>,
    workspace_id: &str,
) -> Result<String, String> {
    let team_id = team_id.ok_or("result.to: team delivery requires a Team owner")?;
    mux.all_tabs()
        .find(|tab| {
            tab.agent_type.is_some()
                && tab.anchors_team()
                && tab.team_id.as_deref() == Some(team_id)
                && mux.find_workspace_for_tab(&tab.id).as_deref() == Some(workspace_id)
        })
        .map(|tab| tab.id.clone())
        .ok_or_else(|| "result.to: Team Orchestrator is unresolved".into())
}

fn chain_summary(spec: &BlueprintSpec) -> String {
    let trigger = match &spec.trigger {
        Trigger::Schedule { every } => format!("schedule:{every}"),
        Trigger::Event { kind, .. } => format!("event:{kind}"),
        Trigger::Input { kind, .. } => format!("input:{}", kind.as_str()),
    };
    let steps = spec.steps.iter().map(|step| {
        let kind = step.kind.name();
        format!("{kind}:{}", step.name)
    });
    std::iter::once(trigger)
        .chain(steps)
        .chain(std::iter::once(format!("result:{:?}", spec.result.to)))
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::BlueprintApplyExt;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    #[test]
    fn blueprint_delivery_unresolvable_named_target_is_refused_at_create_with_reason() {
        let (mut mux, creator) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&creator).unwrap();
        mux.get_tab_mut(&creator).unwrap().cwd = Some(".".into());
        let team = codeterm_types::team::Team::new(&workspace, 1);
        mux.upsert_team(team.clone());
        mux.get_tab_mut(&creator).unwrap().team_id = Some(team.id.clone());
        let fixture = ServiceFixture::new(mux);
        let caller = crate::service_adapter::SentinelCaller {
            tab: creator,
            team: Some(team.id),
            workspace,
            cwd: ".".into(),
            nonce: None,
        };
        let store = crate::store::Store::new(
            fixture.temp.path().join("refusal.json"),
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        );
        let handle = crate::scheduler::fixture_handle_with_ui(store, fixture.service.ui.clone());
        crate::manage_fixture_ports(&fixture.service, &handle);
        let document = json!({"schema_version":2,"kind":"pipeline","name":"create-target-refusal","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo result"}}],"result":{"to":"tab:missing_target"}});
        let error = fixture
            .service
            .blueprint_apply(&caller, document, false)
            .unwrap_err();

        assert!(error.to_string().contains("tab not found"), "{error}");
    }
}
