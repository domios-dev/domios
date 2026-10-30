use super::*;
use codeterm_blueprint_types::{blueprint_document::Document, blueprint_watch as watch};
use std::collections::{BTreeMap, BTreeSet};

impl HandlerAction {
    pub fn from_route(route: &watch::Route) -> Self {
        match route {
            watch::Route::Notify => Self::Notify,
            watch::Route::Stop => Self::Stop,
            watch::Route::Send(target) => Self::Send(target.clone()),
            watch::Route::Cron(target) => Self::Cron(target.clone()),
            watch::Route::Sentinel(target) => Self::Sentinel(target.clone()),
            watch::Route::Command(command) => Self::Command(CommandParameters {
                command: command.command.clone(),
                label: command.label.clone(),
                timeout_ms: command.timeout_ms,
            }),
            watch::Route::RespondPrompt(kind) => Self::RespondPrompt(match kind {
                watch::SafeKind::DirectoryTrust => SafeKind::DirectoryTrust,
                watch::SafeKind::Onboarding => SafeKind::Onboarding,
                watch::SafeKind::Update => SafeKind::Update,
                watch::SafeKind::Permission => SafeKind::Permission,
                watch::SafeKind::Structured => SafeKind::Structured,
                watch::SafeKind::Other(kind) => SafeKind::Other(kind.clone()),
            }),
            watch::Route::Escalate(target) => Self::Escalate(match target {
                watch::EscalateTarget::Creator => EscalateTarget::Creator,
                watch::EscalateTarget::Agent(tab) => EscalateTarget::Agent(tab.clone()),
            }),
        }
    }
}

impl StageDefinition {
    pub fn validate_native_document(&self) -> Result<(), String> {
        let native = self
            .document
            .as_ref()
            .ok_or("Only native v2 Blueprint documents can be persisted")?;
        let runtime = Document::from_definition(native)?
            .lower()
            .map_err(|errors| format!("{errors:?}"))?;
        let matches = if let Some(spec) = runtime.get("spec") {
            let mut spec: codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec =
                serde_json::from_value(spec.clone()).map_err(|e| e.to_string())?;
            if let StageSource::Pipeline(pinned) = &self.source {
                use codeterm_blueprint_types::blueprint_pipeline::StepKind;
                for (authored, installed) in spec.steps.iter_mut().zip(&pinned.steps) {
                    if let (
                        StepKind::Plugin {
                            node_type,
                            revision,
                            outcomes,
                            ..
                        },
                        StepKind::Plugin {
                            node_type: installed_type,
                            revision: installed_revision,
                            outcomes: installed_outcomes,
                            ..
                        },
                    ) = (&mut authored.kind, &installed.kind)
                    {
                        // Apply owns descriptor pins; authoring cannot supply them.
                        if node_type == installed_type && *revision == 0 && *installed_revision > 0
                        {
                            *revision = *installed_revision;
                            if outcomes.is_empty() {
                                outcomes.clone_from(installed_outcomes);
                            }
                        }
                    }
                }
            }
            let interval = match &spec.trigger {
                codeterm_blueprint_types::blueprint_pipeline::Trigger::Schedule { every } => {
                    watch::duration_ms(every)?
                }
                _ => 20_000,
            };
            self.name.as_ref() == Some(&spec.name)
                && self.note == spec.note
                && self.source == StageSource::Pipeline(spec)
                && self.interval_ms == interval
                && self.question.is_empty()
                && self.buckets.is_empty()
                && self.on_other.is_empty()
                && !self.model_classification
                && !self.session
                && self.until.is_none()
                && self.escalation_timeout_ms.is_none()
        } else {
            let create: watch::Create =
                serde_json::from_value(runtime["create"].clone()).map_err(|e| e.to_string())?;
            self.matches_create(create)?
        };
        if matches {
            Ok(())
        } else {
            Err("Native Blueprint document differs from its runtime definition".into())
        }
    }

    fn matches_create(&self, create: watch::Create) -> Result<bool, String> {
        let source = match (create.probe, create.observe, create.input) {
            (Some(command), None, None) => StageSource::Probe(command),
            (None, Some(observation), None) => {
                let binding = match observation {
                    watch::Observation::Tab(reference) => ObserveSource {
                        kind: ObserveKind::Tab,
                        reference,
                    },
                    watch::Observation::CronRun(reference) => ObserveSource {
                        kind: ObserveKind::CronRun,
                        reference,
                    },
                };
                match create.terminal {
                    Some(config) => StageSource::Terminal { binding, config },
                    None => StageSource::Observe(binding),
                }
            }
            (None, None, Some(input)) => StageSource::Input(input),
            _ => return Err("Native Blueprint source is ambiguous".into()),
        };
        let mut handlers = create.handlers.unwrap_or_default();
        let mut used = handlers
            .values()
            .flatten()
            .map(|handler| handler.id.clone())
            .collect::<BTreeSet<_>>();
        for bucket in &create.buckets {
            if !handlers.contains_key(bucket) {
                let id = (1..=65)
                    .map(|ordinal| {
                        codeterm_blueprint_types::automation_name::generated_handler_id(
                            "notify", None, None, bucket, ordinal,
                        )
                    })
                    .find(|id| used.insert(id.clone()))
                    .ok_or("Handler identity budget exceeded")?;
                handlers.insert(
                    bucket.clone(),
                    vec![watch::Definition {
                        id,
                        kind: "notify".into(),
                        parameters: Default::default(),
                    }],
                );
            }
        }
        let handlers = handlers
            .into_iter()
            .map(|(bucket, definitions)| {
                let definitions = definitions
                    .into_iter()
                    .map(|definition| {
                        definition
                            .resolve()
                            .map(|(id, route)| (id, HandlerAction::from_route(&route)))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((bucket, definitions))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let same_handlers = |bucket: &str, actual: &[HandlerDefinition]| {
            let expected = handlers.get(bucket).map_or(&[][..], Vec::as_slice);
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, (id, action))| &actual.id == id && &actual.action == action)
        };
        Ok(self.name == create.name
            && self.note == create.note
            && self.source == source
            && self.question == create.question
            && self.interval_ms == watch::duration_ms(&create.every)?
            && self.session == create.session
            && self.model_classification == create.model_classification
            && self.until == create.until
            && self.escalation_timeout_ms == create.escalation_timeout_ms
            && self.buckets.len() == create.buckets.len()
            && self
                .buckets
                .iter()
                .zip(&create.buckets)
                .all(|(actual, name)| {
                    &actual.name == name
                        && actual.match_pattern.as_ref() == create.matches.get(name)
                        && actual.rearm_on_change == create.rearm_on_change.contains(name)
                        && same_handlers(name, &actual.handlers)
                })
            && same_handlers(OTHER_BUCKET, &self.on_other))
    }
}
