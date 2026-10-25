use super::*;
use serde_json::json;
type LowerResult = Result<Value, Vec<(String, String)>>;
pub(super) fn lower(doc: &Document) -> LowerResult {
    let mut errors = doc.template_errors();
    if matches!(doc.result.to, Some(ResultTarget::Input { .. })) {
        errors.push((
            "result.to".into(),
            "Bind the author result target before lowering".into(),
        ));
    }
    if doc.result.port.is_some() || doc.result.from.is_some() {
        errors.push((
            "result".into(),
            "unsupported: typed result binding runtime".into(),
        ));
    }
    if !doc.inputs.is_empty() {
        errors.push(("inputs".into(), "Bind author inputs before lowering".into()));
    }
    if !doc.imports.is_empty() || !doc.blueprints.is_empty() || !doc.dependencies.is_empty() {
        if let Err(error) = calls::FrozenClosure::from_document(doc.clone()) {
            errors.push(("dependencies".into(), error));
        }
    }
    for (i, node) in doc.nodes().enumerate() {
        let path = format!("steps[{i}]");
        if doc.kind == Kind::Watch && node.limits.is_some() {
            errors.push((format!("{path}.limits"), "unsupported: watch limits".into()));
        }
        if let Some(call) = &node.call {
            if call.pin.is_none() {
                errors.push((path.clone(), "Unresolved call execution".into()));
            }
        }
        if !node.caps.is_empty() && node.call.is_none() {
            errors.push((
                path.clone(),
                "unsupported: per-node capability or budget enforcement".into(),
            ));
        }
        if !node.outcomes.is_empty() {
            errors.push((
                format!("{path}.outcomes"),
                "unsupported: explicit outcome declarations".into(),
            ));
        }
        if doc.kind == Kind::Pipeline
            && (!node.matches.is_empty()
                || node.every.is_some()
                || (node.timeout.is_some() && node.kind() != "command")
                || node.until.is_some()
                || !node.rearm_on_change.is_empty()
                || node.model_classification
                || node.escalation_timeout_ms.is_some()
                || node.session
                || node.paused)
        {
            errors.push((
                path.clone(),
                "unsupported: watch settings on pipeline nodes".into(),
            ));
        }
        for (key, value) in node.params.iter().chain(&node.matches) {
            if value.value().is_none() {
                errors.push((
                    format!("{path}.with.{key}"),
                    "unsupported: unresolved input or runtime data binding".into(),
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let result = match doc.kind {
        Kind::Pipeline => pipeline(doc),
        Kind::Watch => watch(doc),
    };
    result.map_err(|e| vec![(String::new(), e)])
}
fn lit(v: &ValueForm) -> Result<Value, String> {
    v.value()
        .cloned()
        .ok_or("Bind author inputs; runtime data is unsupported here".into())
}
fn params(node: &Node) -> Result<Value, String> {
    Ok(Value::Object(
        node.params
            .iter()
            .map(|(k, v)| Ok((k.clone(), lit(v)?)))
            .collect::<Result<_, String>>()?,
    ))
}
fn route(target: &str) -> String {
    match target {
        "@next" => "next".into(),
        "@stop" => "stop".into(),
        other => other.into(),
    }
}
fn pipeline(doc: &Document) -> Result<Value, String> {
    if !doc.other.is_empty() {
        return Err("unsupported: pipeline other handlers".into());
    }
    let trigger = match doc.trigger.as_ref().ok_or("Pipeline trigger required")? {
        Trigger::Webhook { schema } => json!({"type":"webhook","schema":schema}),
        Trigger::Schedule { every } => json!({"type":"schedule","every":lit(every)?}),
        Trigger::Event { kind, params } => {
            json!({"type":"event","kind":lit(kind)?,"params":params.iter().map(|(k,v)|Ok((k.clone(),lit(v)?))).collect::<Result<BTreeMap<_,_>,String>>()?})
        }
        Trigger::Input { kind, source } => {
            let mut source = lit(source)?;
            if source.is_string() {
                source = json!({"text":source});
            }
            json!({"type":"input","kind":lit(kind)?,"source":source})
        }
    };
    let mut steps = Vec::new();
    for node in &doc.steps {
        if node.call.is_some() {
            steps.push(
                serde_json::to_value(calls::lower::node(doc, node)?).map_err(|e| e.to_string())?,
            );
            continue;
        }
        if node.uses.as_ref().is_some_and(NodeKind::effect) {
            return Err(format!("unsupported: pipeline effect node {}", node.id));
        }
        let mut with = params(node)?;
        if node.run.is_some() && !node.params.is_empty() {
            return Err(format!("unsupported: run parameters on {}", node.id));
        }
        if node.kind() != "ask" && !node.buckets.is_empty() {
            return Err(format!("unsupported: buckets on {}", node.id));
        }
        let mut kind = if let Some(code) = &node.code {
            json!({"kind":"code","code":code})
        } else if let Some(run) = &node.run {
            json!({"kind":"command","command":lit(run)?})
        } else {
            match node.uses.as_ref().ok_or("Node kind required")? {
                NodeKind::Http | NodeKind::Github => json!({"kind":node.kind(),"probe":with}),
                NodeKind::Ask => {
                    with["kind"] = json!("ask");
                    with["buckets"] = json!(node.buckets);
                    with
                }
                NodeKind::Filter | NodeKind::Convert | NodeKind::Agent | NodeKind::Command => {
                    with["kind"] = json!(node.kind());
                    with
                }
                _ => return Err(format!("unsupported: pipeline node {}", node.kind())),
            }
        };
        if node.kind() == "command" {
            if node.params.contains_key("timeout_ms") {
                return Err("Use the command node timeout field instead of with.timeout_ms".into());
            }
            if let Some(timeout) = &node.timeout {
                let timeout = lit(timeout)?;
                let milliseconds = crate::blueprint_watch::duration_ms(
                    timeout
                        .as_str()
                        .ok_or("command timeout must be a duration")?,
                )?;
                if milliseconds > crate::blueprint_pipeline::MAX_COMMAND_STEP_RUNTIME_MS {
                    return Err("command timeout must be positive and at most 60s".into());
                }
                kind["timeout_ms"] = json!(milliseconds);
            }
        }
        let routes = node
            .routes
            .iter()
            .map(|(k, v)| (k.clone(), route(v)))
            .collect::<BTreeMap<_, _>>();
        let execution = crate::blueprint_execution::ExecutionContract {
            input_binding: node.bind.get("in").cloned(),
            input_bindings: bindings::named_bindings(node),
            input: bindings::input_schema(doc, node)?,
            output: node.output.clone(),
            types: doc.types.clone(),
            limits: node.limits.clone(),
        };
        let mut step = json!({"name":node.id,"input":node.input.as_ref().and_then(|p|p.media.as_deref()).filter(|media| *media != "json").unwrap_or("text"),"kind":kind,"on":routes});
        if execution != crate::blueprint_execution::ExecutionContract::default() {
            step["execution"] = json!(execution);
        }
        steps.push(step);
    }
    let spec:crate::blueprint_pipeline::BlueprintSpec=serde_json::from_value(json!({"name":doc.name,"note":doc.note,"owner":doc.owner.as_deref().unwrap_or("team"),"trigger":trigger,"steps":steps,"result":{"to":doc.result.to.as_ref().and_then(ResultTarget::fixed).unwrap_or("owner")}})).map_err(|e|e.to_string())?;
    crate::blueprint_pipeline::validate_parts(
        &spec.name,
        spec.note.as_deref(),
        Some(&spec.trigger),
        &spec.steps.iter().cloned().map(Some).collect::<Vec<_>>(),
        Some(&spec.result),
        None,
    )
    .map_err(|es| {
        es.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    if doc.nodes().any(|node| node.call.is_some()) {
        Ok(json!({"spec":spec,"document":doc}))
    } else {
        Ok(json!({"spec":spec}))
    }
}
fn watch(doc: &Document) -> Result<Value, String> {
    if doc.steps.len() < 2 {
        return Err("A watch needs a source and an ask node".into());
    }
    if doc.owner.is_some() || doc.result != ResultPort::default() {
        return Err(
            "unsupported: watch owner or result declaration; effects control delivery".into(),
        );
    }
    let source = &doc.steps[0];
    let decision = &doc.steps[1];
    if decision.kind() != "ask" {
        return Err("A watch's second node must be ask".into());
    }
    if !source.buckets.is_empty()
        || !source.matches.is_empty()
        || source.until.is_some()
        || !source.rearm_on_change.is_empty()
        || source.model_classification
        || source.escalation_timeout_ms.is_some()
    {
        return Err("unsupported: decision settings on a watch source".into());
    }
    if decision.every.is_some() || decision.timeout.is_some() || decision.session || decision.paused
    {
        return Err("unsupported: source settings on a watch decision".into());
    }
    if !source.routes.is_empty() || !decision.outcomes.is_empty() {
        return Err("unsupported: explicit source routes or watch outcomes".into());
    }
    let mut create = json!({"name":doc.name,"note":doc.note,"question":params(decision)?["question"],"buckets":decision.buckets,"every":source.every.as_ref().map(lit).transpose()?.unwrap_or(json!("20s")),"timeout":source.timeout.as_ref().map(lit).transpose()?,"paused":source.paused,"session":source.session,"matches":decision.matches.iter().map(|(k,v)|Ok((k.clone(),lit(v)?))).collect::<Result<BTreeMap<_,_>,String>>()?,"until":decision.until,"rearm_on_change":decision.rearm_on_change,"model_classification":decision.model_classification,"escalation_timeout_ms":decision.escalation_timeout_ms});
    if decision.params.keys().any(|k| k != "question") {
        return Err("unsupported: watch decision parameters".into());
    }
    let with = params(source)?;
    let allowed: &[&str] = match source.uses.as_ref() {
        Some(NodeKind::Probe) => &["command"],
        Some(NodeKind::Observe) => &["kind", "id"],
        Some(NodeKind::Terminal) => &["kind", "id", "pattern", "idle_ms"],
        Some(NodeKind::Input) => &[
            "kind",
            "file",
            "url",
            "text",
            "convert_to",
            "convert_timeout_ms",
        ],
        _ => &[],
    };
    if source
        .params
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err("unsupported: watch source parameters".into());
    }
    if decision
        .routes
        .keys()
        .any(|key| key != "other" && !decision.buckets.contains(key))
    {
        return Err("unsupported: watch decision routes outside declared buckets".into());
    }

    match source.uses.as_ref().ok_or("Watch source kind required")? {
        NodeKind::Probe => create["probe"] = with["command"].clone(),
        NodeKind::Observe | NodeKind::Terminal => {
            let kind = with["kind"]
                .as_str()
                .ok_or("Observe requires kind: tab or cron_run")?;
            create["observe"] = json!({"kind":kind,"id":with["id"]});
            if source.uses == Some(NodeKind::Terminal) {
                create["terminal"] = json!({"pattern":with["pattern"],"idle_ms":with["idle_ms"]});
            }
        }
        NodeKind::Input => {
            let data = if with.get("file").is_some() {
                json!({"file":with["file"]})
            } else if with.get("url").is_some() {
                json!({"url":with["url"]})
            } else {
                json!({"text":with["text"]})
            };
            create["input"] = json!({"kind":with["kind"],"data":data,"convert_to":with["convert_to"],"convert_timeout_ms":with["convert_timeout_ms"]});
        }
        _ => return Err(format!("unsupported: watch source {}", source.kind())),
    }
    let all = doc.nodes().collect::<Vec<_>>();
    let mut handlers = BTreeMap::new();
    let mut reached = std::collections::BTreeSet::new();
    for bucket in decision
        .buckets
        .iter()
        .chain(std::iter::once(&"other".into()))
    {
        let Some(target) = decision.routes.get(bucket) else {
            continue;
        };
        let mut target = target.as_str();
        let mut chain = Vec::new();
        let mut previous = 1;
        while !matches!(target, "stop" | "@stop") {
            let (index, node) = all
                .iter()
                .enumerate()
                .find(|(_, n)| n.id == target)
                .ok_or("Watch route must name an effect or @stop")?;
            if index <= previous {
                return Err("Watch effects must route forward".into());
            }
            previous = index;
            reached.insert(node.id.clone());
            if !node.buckets.is_empty()
                || !node.matches.is_empty()
                || node.every.is_some()
                || node.timeout.is_some()
                || node.until.is_some()
                || !node.rearm_on_change.is_empty()
                || node.model_classification
                || node.escalation_timeout_ms.is_some()
                || node.session
                || node.paused
            {
                return Err(format!("unsupported: watch settings on effect {}", node.id));
            }
            let with = params(node)?;
            if !node.uses.as_ref().is_some_and(NodeKind::effect) && node.kind() != "command" {
                return Err(format!("unsupported: watch effect {}", node.kind()));
            }
            if with.as_object().is_some_and(|p| {
                p.keys()
                    .any(|k| !matches!(k.as_str(), "target" | "command" | "label" | "timeout_ms"))
            }) {
                return Err(format!(
                    "unsupported: effect data parameters on {}",
                    node.id
                ));
            }
            let mut parameters = with;
            if let Some(run) = &node.run {
                parameters["command"] = lit(run)?;
            }
            chain.push(json!({"id":node.id,"type":node.kind(),"parameters":parameters}));
            if node.routes.keys().any(|k| k != "done") {
                return Err("unsupported: watch effect outcome routes".into());
            }
            target = node
                .routes
                .get("done")
                .map(String::as_str)
                .unwrap_or("@stop");
            if matches!(target, "next" | "@next") {
                target = all.get(index + 1).map_or("@stop", |n| n.id.as_str());
            }
        }
        handlers.insert(bucket.clone(), chain);
    }
    if all.iter().skip(2).any(|n| !reached.contains(&n.id)) {
        return Err("unsupported: unreferenced watch effect".into());
    }
    create["handlers"] = json!(handlers);
    let mut create: crate::blueprint_watch::Create =
        serde_json::from_value(create).map_err(|e| e.to_string())?;
    create.normalize_handlers();
    let errors = create.admission_errors();
    if !errors.is_empty() {
        return Err(format!("{errors:?}"));
    }
    Ok(json!({"create":create}))
}
