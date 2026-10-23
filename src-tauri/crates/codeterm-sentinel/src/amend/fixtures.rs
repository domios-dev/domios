use super::*;
use crate::{
    handlers::{self, Definition},
    machine,
};
use codeterm_blueprint_types::sentinel_input::{DataRef, MediaKind};

/// Amendable part of an input source; `kind` is optional and must name the immutable kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPatch {
    #[serde(default)]
    pub kind: Option<MediaKind>,
    pub data: DataRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convert_to: Option<MediaKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convert_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub name: Option<String>,
    pub question: String,
    pub buckets: Vec<String>,
    #[serde(default)]
    pub every: Option<String>,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    #[serde(default)]
    pub input: Option<InputPatch>,
    pub handlers: Option<BTreeMap<String, Vec<Definition>>>,
    #[serde(default)]
    pub on_other: Vec<Definition>,
    #[serde(default)]
    pub escalation_timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_transitions: Option<u16>,
    #[serde(default)]
    pub max_actions: Option<u16>,
    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Update {
    pub fn prepare(
        self,
        current: &Sentinel,
        author_tab_id: &str,
        connected: bool,
    ) -> Result<PreparedUpdate, String> {
        let name = self.name.as_deref().ok_or_else(|| {
            "Blueprint update definition needs a name; use a short name such as ci-green-watch"
                .to_string()
        })?;
        codeterm_blueprint_types::automation_name::validate_sentinel_name(name)?;
        if self.question.trim().is_empty() || self.question.len() > 4096 {
            return Err("question must be nonempty and bounded to 4096 bytes".into());
        }
        let interval_ms = match self.every.as_deref() {
            Some(every) => crate::store::duration_ms(every)?,
            None if current.input().is_some() => current.interval_ms,
            None => return Err("every is required for a scheduled blueprint".into()),
        };
        machine::validate(
            &self.buckets,
            &self.matches,
            connected || !self.model_classification,
            interval_ms,
        )?;
        if self
            .rearm_on_change
            .iter()
            .any(|bucket| !self.buckets.contains(bucket))
        {
            return Err("rearm_on_change must name declared buckets".into());
        }
        if self
            .until
            .as_ref()
            .is_some_and(|bucket| !self.buckets.contains(bucket))
        {
            return Err("until must name a declared bucket".into());
        }
        let used_transitions = if matches!(
            current.observe(),
            Some(crate::source::TabBinding::CronRun(_))
        ) {
            usize::from(current.run_transitions)
        } else {
            current
                .transitions
                .len()
                .saturating_sub(current.cleanup.transition_offset)
        };
        let max_transitions = self.max_transitions.unwrap_or(current.max_transitions);
        let max_actions = self.max_actions.unwrap_or(current.max_actions);
        if max_transitions == 0
            || max_transitions > 256
            || (!current.machine.stopped && usize::from(max_transitions) < used_transitions)
            || max_actions == 0
            || max_actions > 1024
            || (!current.machine.stopped && usize::from(max_actions) < claimed_actions(current))
        {
            return Err(
                "blueprint budgets must stay within limits and cannot shrink below use".into(),
            );
        }
        if self
            .cause
            .as_deref()
            .is_some_and(|cause| !bounded_id(cause))
        {
            return Err("cause must be a bounded transition ID".into());
        }
        if self
            .note
            .as_deref()
            .is_some_and(|note| note.chars().count() > 200 || note.chars().any(char::is_control))
        {
            return Err("note must be bounded display text".into());
        }
        let routes = handlers::definitions(&self.buckets, self.routes, self.handlers)?;
        let mut other = BTreeMap::new();
        other.insert("other".into(), self.on_other);
        let on_other = handlers::definitions(&["other".into()], BTreeMap::new(), Some(other))?
            .remove("other")
            .unwrap_or_default();
        let input = amended_input(&current.input().cloned(), self.input)?;
        let mut job = current.clone();
        job.name = self.name;
        job.note = self.note.clone().or_else(|| current.note.clone());
        job.question = self.question;
        job.buckets = self.buckets;
        job.interval_ms = interval_ms;
        job.routes = routes;
        job.matches = self.matches;
        job.model_classification = self.model_classification;
        job.rearm_on_change = self.rearm_on_change;
        job.until = self.until;
        job.set_input(input);
        job.max_transitions = max_transitions;
        job.max_actions = max_actions;
        job.on_other = on_other;
        job.escalation_timeout_ms = self.escalation_timeout_ms;
        crate::source::validate(&job)?;
        crate::escalation::validate(&job)?;
        let snapshot = DefinitionSnapshot::from_job(&job);
        if serde_json::to_vec(&snapshot)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_UPDATE_BYTES
        {
            return Err("blueprint definition exceeds 64 KiB".into());
        }
        if author_tab_id != current.creator_tab_id && has_new_command(current, &job) {
            return Err("only the blueprint creator may introduce new command text".into());
        }
        job.document = Some(native_fixture(&job)?);
        Ok(PreparedUpdate {
            job,
            cause_transition_id: self.cause,
            note: self.note,
        })
    }
}

fn claimed_actions(job: &Sentinel) -> usize {
    job.transitions
        .iter()
        .flat_map(|transition| &transition.handlers)
        .filter(|handler| handler.claim_id.is_some())
        .count()
        .saturating_sub(job.cleanup.action_offset)
}

/// The input after one amendment: the kind is immutable, the data and conversion are not.
fn amended_input(
    current: &Option<InputSpec>,
    patch: Option<InputPatch>,
) -> Result<Option<InputSpec>, String> {
    match (current, patch) {
        (Some(current_input), Some(patch)) => {
            if patch.kind.is_some_and(|kind| kind != current_input.kind) {
                return Err("input kind is immutable".into());
            }
            let next = InputSpec {
                kind: current_input.kind,
                data: patch.data,
                convert_to: patch.convert_to,
                convert_timeout_ms: patch.convert_timeout_ms,
            };
            codeterm_blueprint_types::sentinel_input::validate(&next)
                .map_err(|error| format!("input: {error}"))?;
            Ok(Some(next))
        }
        (Some(current_input), None) => Ok(Some(current_input.clone())),
        (None, Some(_)) => Err("input source is immutable; this stage has no input".into()),
        (None, None) => Ok(None),
    }
}

fn native_fixture(job: &Sentinel) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let original = job
        .document
        .as_ref()
        .ok_or("Fixture requires a native document")?;
    let mut source = original["steps"][0].clone();
    source["every"] = json!({"lit":format!("{}s",job.interval_ms / 1000)});
    let decision = json!({"with":{"question":{"lit":job.question}},"buckets":job.buckets,
        "matches":job.matches.iter().map(|(key,value)|(key.clone(),json!({"lit":value}))).collect::<BTreeMap<_,_>>(),
        "model_classification":job.model_classification,"rearm_on_change":job.rearm_on_change,"until":job.until,"escalation_timeout_ms":job.escalation_timeout_ms});
    let mut effects = BTreeMap::new();
    for (bucket, handlers) in job
        .routes
        .iter()
        .chain(std::iter::once((&"other".into(), &job.on_other)))
    {
        let chain = handlers.iter().map(|handler| {
            let params = match &handler.action {
                Route::Notify | Route::Stop => json!({}),
                Route::Send(target) | Route::Cron(target) | Route::Sentinel(target) => json!({"target":target}),
                Route::Escalate(crate::machine::EscalateTarget::Creator) => json!({"target":"creator"}),
                Route::Escalate(crate::machine::EscalateTarget::Agent(tab)) => json!({"target":format!("agent:{tab}")}),
                Route::RespondPrompt(kind) => json!({"target":kind.as_str()}),
                Route::Command(command) => json!({"command":command.command,"label":command.label,"timeout_ms":command.timeout_ms}),
            };
            json!({"id":handler.id,"uses":handler.action.name(),"with":crate::test_support::literal_params(params)})
        }).collect::<Vec<_>>();
        effects.insert(bucket.clone(), chain);
    }
    let document = crate::test_support::watch(
        json!(job.name),
        source,
        decision,
        json!(effects),
        json!(job.note),
    );
    codeterm_blueprint_types::blueprint_document::Document::from_definition(&document)?;
    Ok(document)
}
