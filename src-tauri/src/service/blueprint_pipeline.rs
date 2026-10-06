use codeterm_types::blueprint_pipeline::{
    ensure_trigger_available, BlueprintOwner, BlueprintSpec, ConverterEdge, ResultTarget, StepKind,
    Trigger, ValidationError,
};
use serde_json::{json, Value};
use tauri::Manager;

use crate::sentinel::scheduler::Handle;

impl super::ServiceContext {
    pub(crate) fn blueprint_log(
        &self,
        caller: &super::SentinelCaller,
        id: &str,
        limit: u16,
    ) -> Result<Vec<crate::storage::BlueprintJournalEntry>, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
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
            .wait(crate::sentinel::storage::COMMIT_DEADLINE)
            .map(|read| read.value)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn blueprint_pipeline_validate(
        &self,
        caller: &super::SentinelCaller,
        spec: BlueprintSpec,
    ) -> Result<Value, String> {
        let errors = self.pipeline_validation_errors(&spec)?;
        let trigger_available =
            !codeterm_types::blueprint_pipeline::trigger_unavailable(&spec.trigger);
        let mut checks = vec![
            field_check("name", &errors),
            field_check("trigger", &errors),
        ];
        if !trigger_available {
            checks[1]["passed"] = json!(false);
            checks[1]["reason"] = json!("trigger_unavailable");
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
                StepKind::Command { .. } => {}
            }
        }
        checks.push(field_check("result.to", &errors));
        let owner_available = spec.owner != BlueprintOwner::Team || caller.team.is_some();
        checks.push(json!({
            "field":"owner",
            "passed":owner_available,
            "reason":if owner_available { Value::Null } else { json!("Team owner requires the creating tab to belong to a Team") }
        }));
        let (caller_live, result_scope) = {
            let mux = self.mux.read();
            let live = super::sentinel::caller_is_live(&mux, caller);
            let target = live
                .then(|| validate_pipeline_result_scope(&mux, caller, &spec.result.to))
                .unwrap_or_else(|| Err("blueprint caller scope changed".into()));
            (live, target)
        };
        checks.push(json!({"field":"scope", "passed":caller_live,
            "reason":if caller_live { Value::Null } else { json!("blueprint caller scope changed") }}));
        checks.push(
            json!({"field":"result.scope", "passed":result_scope.is_ok(),
            "reason":result_scope.err()}),
        );
        Ok(json!({
            "normalized_spec": spec,
            "chain": chain_summary(&spec),
            "checks": checks
        }))
    }

    pub(crate) fn blueprint_pipeline_create(
        &self,
        caller: &super::SentinelCaller,
        spec: BlueprintSpec,
    ) -> Result<Value, String> {
        self.validate_pipeline(&spec)?;
        ensure_trigger_available(&spec.trigger)
            .map_err(|error| format!("{}: {}", error.field, error.reason))?;
        if spec.owner == BlueprintOwner::Team && caller.team.is_none() {
            return Err("owner: --owner team requires the creating tab to belong to a Team".into());
        }
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let mut job = handle.store.lock().prepare_pipeline(
            spec.clone(),
            crate::sentinel::store::Owner {
                creator_tab_id: caller.tab.clone(),
                team_id: caller.team.clone(),
                workspace_id: caller.workspace.clone(),
                cwd: caller.cwd.clone(),
            },
            true,
        )?;
        {
            let mux = self.mux.read();
            if !super::sentinel::caller_is_live(&mux, caller) {
                return Err("blueprint creator or Team is gone".into());
            }
            validate_result_scope(&mux, &job, &spec.result.to)?;
            let tab = mux.get_tab(&caller.tab).ok_or("blueprint creator gone")?;
            job.agent_authored = tab.agent_type.is_some() && tab.session_binding_nonce.is_some();
        }
        let staged = handle.store.lock().stage_insert(job.clone());
        staged.settle(&handle.store)?;
        handle.notify();
        let owner = match spec.owner {
            BlueprintOwner::Tab => json!({"kind":"tab", "id":caller.tab}),
            BlueprintOwner::Team => json!({"kind":"team", "id":caller.team}),
        };
        Ok(json!({
            "id": job.id,
            "owner": owner,
            "chain": chain_summary(&spec),
            "blueprint": job
        }))
    }

    pub(crate) fn blueprint_pipeline_validate_update(
        &self,
        caller: &super::SentinelCaller,
        id: &str,
        expect_version: u64,
        spec: BlueprintSpec,
    ) -> Result<Value, String> {
        let current = self.pipeline_update_current(caller, id, expect_version)?;
        let mut preview = self.blueprint_pipeline_validate(caller, spec.clone())?;
        let owner_matches =
            current.pipeline.as_ref().map(|pipeline| pipeline.owner) == Some(spec.owner);
        preview["checks"].as_array_mut().unwrap().push(json!({
            "field":"owner",
            "passed":owner_matches,
            "reason":if owner_matches { Value::Null } else { json!("pipeline owner is immutable") }
        }));
        preview["id"] = json!(id);
        preview["expect_version"] = json!(expect_version);
        Ok(preview)
    }

    pub(crate) fn blueprint_pipeline_update(
        &self,
        caller: &super::SentinelCaller,
        id: &str,
        expect_version: u64,
        spec: BlueprintSpec,
        cause: Option<String>,
    ) -> Result<crate::sentinel::store::Sentinel, String> {
        let current = self.pipeline_update_current(caller, id, expect_version)?;
        self.validate_pipeline(&spec)?;
        ensure_trigger_available(&spec.trigger)
            .map_err(|error| format!("{}: {}", error.field, error.reason))?;
        if current.pipeline.as_ref().map(|pipeline| pipeline.owner) != Some(spec.owner) {
            return Err("owner: pipeline owner is immutable".into());
        }
        let codeterm_types::blueprint_pipeline::Trigger::Schedule { every } = &spec.trigger else {
            return Err("trigger_unavailable".into());
        };
        {
            let mux = self.mux.read();
            if !super::sentinel::caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            validate_result_scope(&mux, &current, &spec.result.to)?;
        }
        let mut next = current.clone();
        next.name = Some(spec.name.clone());
        next.interval_ms = crate::sentinel::store::duration_ms(every)?;
        next.pipeline = Some(spec);
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let authored_at = handle.store.lock().clock.now_ms();
        let staged = handle.store.lock().stage_update_definition(
            expect_version,
            &caller.tab,
            authored_at,
            crate::sentinel::amend::PreparedUpdate {
                job: next,
                cause_transition_id: cause,
                note: None,
            },
        );
        let updated = staged.settle(&handle.store)?;
        handle.notify();
        Ok(updated)
    }

    fn pipeline_update_current(
        &self,
        caller: &super::SentinelCaller,
        id: &str,
        expect_version: u64,
    ) -> Result<crate::sentinel::store::Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let store = handle.store.lock();
        store.ensure_available()?;
        let current = store
            .get(id)
            .filter(|job| caller.can_amend(job))
            .cloned()
            .ok_or_else(|| format!("blueprint not found in this workspace and Team: {id}"))?;
        if current.pipeline.is_none() {
            return Err("blueprint has no pipeline definition".into());
        }
        if current.definition_version != expect_version {
            return Err(crate::sentinel::amend::version_conflict(
                current.definition_version,
            ));
        }
        if current.machine.stopped
            || current
                .expires_at
                .is_some_and(|at| store.clock.now_ms() >= at)
        {
            return Err("stopped blueprints cannot be amended".into());
        }
        Ok(current)
    }

    pub(crate) fn blueprint_pipeline_nodes(&self) -> Value {
        let mut nodes = vec![
            json!({"kind":"trigger", "type":"schedule", "title":"Schedule", "params_schema":{"every":{"type":"duration","minimum":"10s"}}}),
            json!({"kind":"trigger", "type":"event", "title":"Event", "params_schema":{"source":"event source kind","params":"event source schema"}}),
            json!({"kind":"trigger", "type":"input", "title":"Input", "params_schema":{"kind":"text|file|audio|video|image","source":"text|file|url","convert_to":"optional media kind"}}),
            json!({"kind":"step", "type":"command", "title":"Command", "params_schema":{"command":{"type":"string","max_length":16384}}}),
            json!({"kind":"step", "type":"ask", "title":"Ask", "params_schema":{"question":"string","buckets":"declared string array"}}),
            json!({"kind":"step", "type":"filter", "title":"Filter", "params_schema":{"pattern":"regex","bucket":"optional bucket"}}),
            json!({"kind":"step", "type":"convert", "title":"Convert", "params_schema":{"from":"media kind","to":"media kind"}}),
        ];
        for (_, conversion) in self.converter_registry.available() {
            nodes.push(json!({
                "kind":"converter",
                "from":conversion.from.as_str(),
                "to":conversion.to.as_str(),
                "title":format!("{} to {}", conversion.from.as_str(), conversion.to.as_str()),
                "params_schema":{"from":conversion.from.as_str(), "to":conversion.to.as_str()}
            }));
        }
        let event_sources = self.graph_node_registry.event_sources();
        for source in &event_sources {
            let schemas = source.to_value();
            nodes.push(json!({
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

    fn pipeline_validation_errors(
        &self,
        spec: &BlueprintSpec,
    ) -> Result<Vec<ValidationError>, String> {
        let definition_bytes = serde_json::to_vec(spec).map_err(|error| error.to_string())?;
        if definition_bytes.len() > 65_536 {
            return Ok(vec![ValidationError {
                field: "spec".into(),
                reason: "blueprint definition exceeds 64 KiB".into(),
            }]);
        }
        let converters = self
            .converter_registry
            .available()
            .into_iter()
            .map(|(_, edge)| ConverterEdge {
                from: edge.from,
                to: edge.to,
            })
            .collect::<Vec<_>>();
        let mut errors = codeterm_types::blueprint_pipeline::validate(spec, &converters)
            .err()
            .unwrap_or_default();
        if let Trigger::Input {
            kind,
            convert_to: Some(to),
            ..
        } = &spec.trigger
        {
            if self.converter_registry.plan(*kind, *to).is_err()
                && !errors
                    .iter()
                    .any(|error| error.field == "trigger.convert_to")
            {
                errors.push(ValidationError {
                    field: "trigger.convert_to".into(),
                    reason: "no converter chain is available".into(),
                });
            }
        }
        for (index, step) in spec.steps.iter().enumerate() {
            if let StepKind::Convert { from, to } = &step.kind {
                let field = format!("steps[{index}].convert");
                if self.converter_registry.plan(*from, *to).is_err()
                    && !errors.iter().any(|error| error.field == field)
                {
                    errors.push(ValidationError {
                        field,
                        reason: "no converter chain is available".into(),
                    });
                }
            }
        }
        Ok(errors)
    }
}

fn field_check(field: &str, errors: &[ValidationError]) -> Value {
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
    json!({
        "field":field,
        "passed":failures.is_empty(),
        "reason":if failures.is_empty() { Value::Null } else { json!(failures.join("; ")) }
    })
}

fn validate_pipeline_result_scope(
    mux: &crate::multiplexer::state::MultiplexerState,
    caller: &super::SentinelCaller,
    target: &ResultTarget,
) -> Result<(), String> {
    match target {
        ResultTarget::Owner | ResultTarget::Notify => Ok(()),
        ResultTarget::Team if caller.team.is_none() => {
            Err("result.to: team delivery requires a Team owner".into())
        }
        ResultTarget::Team => Ok(()),
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

fn validate_result_scope(
    mux: &crate::multiplexer::state::MultiplexerState,
    job: &crate::sentinel::store::Sentinel,
    target: &ResultTarget,
) -> Result<(), String> {
    match target {
        ResultTarget::Owner | ResultTarget::Notify => Ok(()),
        ResultTarget::Team if job.team_id.is_none() => {
            Err("result.to: team delivery requires a Team owner".into())
        }
        ResultTarget::Team => Ok(()),
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

fn chain_summary(spec: &BlueprintSpec) -> String {
    let trigger = match &spec.trigger {
        Trigger::Schedule { every } => format!("schedule:{every}"),
        Trigger::Event { source, .. } => format!("event:{source}"),
        Trigger::Input { kind, .. } => format!("input:{}", kind.as_str()),
    };
    let steps = spec.steps.iter().map(|step| {
        let kind = match &step.kind {
            StepKind::Command { .. } => "command",
            StepKind::Ask { .. } => "ask",
            StepKind::Filter { .. } => "filter",
            StepKind::Convert { .. } => "convert",
        };
        format!("{kind}:{}", step.name)
    });
    std::iter::once(trigger)
        .chain(steps)
        .chain(std::iter::once(format!("result:{:?}", spec.result.to)))
        .collect::<Vec<_>>()
        .join(" -> ")
}
