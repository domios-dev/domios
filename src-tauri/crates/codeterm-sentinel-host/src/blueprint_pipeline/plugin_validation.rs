use super::*;
use codeterm_plugin::{blueprint_step::*, graph_nodes::GraphNodeRegistry};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn validate(service: &ServiceContext, spec: &BlueprintSpec) -> Vec<ValidationError> {
    let registry = service.ext::<Arc<GraphNodeRegistry>>();
    let mut errors = Vec::new();
    let mut ceilings = BTreeMap::new();
    for (index, step) in spec.steps.iter().enumerate() {
        if let StepKind::Agent { provider, .. } = &step.kind {
            for binding in step
                .execution
                .as_ref()
                .into_iter()
                .flat_map(|execution| execution.input_bindings.values())
            {
                let Ok(binding) =
                    codeterm_blueprint_types::blueprint_document::bindings::parse(binding)
                else {
                    continue;
                };
                let Some(source) = spec.steps.iter().find(|source| source.name == binding.node)
                else {
                    continue;
                };
                let StepKind::Plugin { node_type, .. } = &source.kind else {
                    continue;
                };
                let Some(source) = registry.step(node_type) else {
                    continue;
                };
                let Some(port) = source.descriptor.outputs.get(binding.selection) else {
                    continue;
                };
                let modality = match port.kind {
                    PortKind::Image => Some(codeterm_plugin::agent_task::AgentTaskMediaKind::Image),
                    PortKind::Audio => Some(codeterm_plugin::agent_task::AgentTaskMediaKind::Audio),
                    PortKind::Video => Some(codeterm_plugin::agent_task::AgentTaskMediaKind::Video),
                    PortKind::File => Some(codeterm_plugin::agent_task::AgentTaskMediaKind::File),
                    _ => None,
                };
                if modality.is_some_and(|modality| {
                    service
                        .chatbackend_registry
                        .get(provider)
                        .is_none_or(|backend| !backend.agent_task_media().contains(&modality))
                }) {
                    errors.push(ValidationError::new(
                        ValidationPath::Step {
                            index,
                            field: StepField::Parameters("agent".into()),
                        },
                        "agent backend does not accept the bound media modality",
                    ));
                }
            }
        }
        let StepKind::Plugin {
            node_type,
            config,
            params,
            outcomes,
        } = &step.kind
        else {
            continue;
        };
        let field = ValidationPath::Step {
            index,
            field: StepField::Common("plugin".into()),
        };
        let Some(registered) = registry.step(node_type) else {
            errors.push(ValidationError::new(
                field.clone(),
                "plugin_step_unavailable",
            ));
            continue;
        };
        let descriptor = &registered.descriptor;
        if descriptor
            .inputs
            .values()
            .chain(descriptor.outputs.values())
            .any(|port| port.kind == PortKind::File)
        {
            errors.push(ValidationError::new(
                field.clone(),
                "plugin file media delivery is unavailable",
            ));
        }
        if descriptor.revision != config.revision {
            errors.push(ValidationError::new(
                field.clone(),
                "plugin_step_revision_changed",
            ));
        }
        if descriptor.params.validate_instance(params).is_err() {
            errors.push(ValidationError::new(
                field.clone(),
                "plugin_step_parameters_violate_schema",
            ));
        }
        if outcomes
            .iter()
            .any(|outcome| !descriptor.outcomes.contains(outcome))
        {
            errors.push(ValidationError::new(
                field.clone(),
                "plugin_step_outcome_not_declared",
            ));
        }
        if ceilings
            .insert(&config.spending.unit, config.spending.run_ceiling)
            .is_some_and(|ceiling| ceiling != config.spending.run_ceiling)
        {
            errors.push(ValidationError::new(
                field.clone(),
                "run ceilings for the same unit must agree",
            ));
        }
        let bindings = step
            .execution
            .as_ref()
            .map(|contract| &contract.input_bindings);
        if let Some(bindings) = bindings.filter(|bindings| !bindings.is_empty()) {
            for (port, binding) in bindings {
                let Some(wanted) = descriptor.inputs.get(port) else {
                    errors.push(ValidationError::new(
                        field.clone(),
                        "undeclared plugin input port",
                    ));
                    continue;
                };
                let checked = (|| {
                    let binding =
                        codeterm_blueprint_types::blueprint_document::bindings::parse(binding)
                            .ok()?;
                    let source_index = spec
                        .steps
                        .iter()
                        .position(|step| step.name == binding.node)?;
                    if source_index >= index {
                        return None;
                    }
                    let source = &spec.steps[source_index];
                    let StepKind::Plugin { node_type, .. } = &source.kind else {
                        let output = source.execution.as_ref()?.output.as_ref()?;
                        let actual = match output.media.as_deref() {
                            Some("text") => PortKind::Text,
                            Some("json") | None => PortKind::Json,
                            _ => return None,
                        };
                        return (binding.selection.is_empty()
                            && actual == wanted.kind
                            && wanted.min_items <= 1
                            && wanted.max_items >= 1)
                            .then_some(());
                    };
                    let source = registry.step(node_type)?;
                    let actual = source.descriptor.outputs.get(binding.selection)?;
                    (actual.kind == wanted.kind
                        && actual.min_items >= wanted.min_items
                        && actual.max_items <= wanted.max_items
                        && wanted
                            .schema
                            .as_ref()
                            .is_none_or(|schema| actual.schema.as_ref() == Some(schema)))
                    .then_some(())
                })();
                if checked.is_none() {
                    errors.push(ValidationError::new(
                        field.clone(),
                        "plugin port binding is unavailable or incompatible",
                    ));
                }
            }
            if descriptor
                .inputs
                .iter()
                .any(|(name, port)| port.min_items > 0 && !bindings.contains_key(name))
            {
                errors.push(ValidationError::new(
                    field.clone(),
                    "required plugin input port is unbound",
                ));
            }
        } else if descriptor.inputs.len() > 1
            && (descriptor.inputs.values().any(|port| port.min_items > 0)
                || step
                    .execution
                    .as_ref()
                    .is_some_and(|execution| execution.input_binding.is_some()))
        {
            errors.push(ValidationError::new(
                field.clone(),
                "multiple plugin inputs require named bindings",
            ));
        }
    }
    errors
}
