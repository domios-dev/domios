use super::*;
use codeterm_plugin::{blueprint_step::*, graph_nodes::GraphNodeRegistry};
use std::sync::Arc;

pub(super) fn validate(service: &ServiceContext, spec: &BlueprintSpec) -> Vec<ValidationError> {
    let registry = service.ext::<Arc<GraphNodeRegistry>>();
    let mut errors = Vec::new();
    for (index, step) in spec.steps.iter().enumerate() {
        if !matches!(step.kind, StepKind::Plugin { .. }) {
            if let Some(contract) = &step.execution {
                for binding in contract
                    .input_binding
                    .iter()
                    .chain(contract.input_bindings.values())
                {
                    let Ok(binding) =
                        codeterm_blueprint_types::blueprint_document::bindings::parse(binding)
                    else {
                        continue;
                    };
                    let Some(source) = spec.steps[..index]
                        .iter()
                        .find(|source| source.name == binding.node)
                    else {
                        continue;
                    };
                    let StepKind::Plugin { node_type, .. } = &source.kind else {
                        continue;
                    };
                    if registry.step(node_type).is_some_and(|source| {
                        !source.descriptor.outputs.contains_key(binding.selection)
                    }) {
                        errors.push(ValidationError::new(
                            ValidationPath::Step {
                                index,
                                field: StepField::Common("bind".into()),
                            },
                            "undeclared plugin output port",
                        ));
                    }
                }
            }
        }
        let StepKind::Plugin {
            node_type,
            revision,
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
        if *revision != 0 && descriptor.revision != *revision {
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
