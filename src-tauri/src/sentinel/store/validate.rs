//! Definition admission: stored-inventory invariants and `Create` preparation.

use super::*;

pub fn duration_ms(value: &str) -> Result<u64, String> {
    if value.len() > 32
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd'))
    {
        return Err("duration: use seconds, minutes, hours or days, e.g. 20s or 1h30m".into());
    }
    if value.chars().any(|c| c.is_ascii_alphabetic())
        && value.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("duration: every component needs a unit".into());
    }
    let mut number = String::new();
    let mut bound = 0u64;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().map_err(|_| "duration: invalid number")?;
        number.clear();
        let factor = match c {
            's' => 1000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => unreachable!(),
        };
        bound = bound
            .checked_add(n.checked_mul(factor).ok_or("duration: overflow")?)
            .ok_or("duration: overflow")?;
    }
    if !number.is_empty() {
        bound = bound
            .checked_add(
                number
                    .parse::<u64>()
                    .map_err(|_| "duration: invalid number")?
                    .checked_mul(60_000)
                    .ok_or("duration: overflow")?,
            )
            .ok_or("duration: overflow")?;
    }
    if bound == 0 || bound > 366 * 86_400_000 {
        return Err("duration: must be positive and at most 366 days".into());
    }
    crate::cron::store::parse_oneshot_schedule_at(&format!("in {value}"), 0)
}

/// Every stored-inventory invariant, shared by the legacy file and typed Storage loads.
pub(in crate::sentinel) fn validate(jobs: Vec<Sentinel>) -> Result<Vec<Sentinel>, String> {
    if jobs.len() > MAX_SENTINELS
        || jobs
            .iter()
            .filter(|job| !job.machine.stopped && !job.machine.paused)
            .count()
            > MAX_ACTIVE
    {
        return Err("blueprint store exceeds capacity".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for job in &jobs {
        if job.pipeline().is_none() {
            machine::validate(&job.buckets, &job.matches, true, job.interval_ms)?;
        }
        if job
            .rearm_on_change
            .iter()
            .any(|bucket| !job.buckets.contains(bucket))
        {
            return Err("rearm_on_change must name declared buckets".into());
        }
        handlers::validate(&job.buckets, &job.routes)?;
        source::validate(job)?;
        escalation::validate(job)?;
        amend::validate(job)?;
        if job.max_transitions == 0
            || job.max_transitions > 256
            || job.max_actions == 0
            || job.max_actions > 1024
            || job.transitions.len() > MAX_LIFETIME_TRANSITIONS
            || (!job.session && job.run_transitions > job.max_transitions)
            || (!job.session
                && !matches!(job.observe(), Some(TabBinding::CronRun(_)))
                && job.transitions.len() > usize::from(job.max_transitions))
            || job
                .transitions
                .iter()
                .flat_map(|t| &t.handlers)
                .filter(|r| r.claim_id.is_some())
                .count()
                > super::action_limit(job)
        {
            return Err("invalid blueprint budgets or retention".into());
        }
        let mut transitions = std::collections::BTreeSet::new();
        for transition in &job.transitions {
            let declared = job
                .declared_handlers(&transition.bucket)
                .ok_or("stored transition has an undeclared bucket")?;
            if transition.id.is_empty()
                || transition.id.len() > 128
                || !transitions.insert(&transition.id)
                || transition.handlers.len() > declared.len()
            {
                return Err("invalid stored transition identity or handler retention".into());
            }
            let mut results = std::collections::BTreeSet::new();
            for result in &transition.handlers {
                if let Some(command) = &result.command_result {
                    command.validate()?;
                    let handler = declared
                        .iter()
                        .find(|handler| handler.id == result.handler_id)
                        .ok_or("command result has no declared handler")?;
                    let expected = match command.status {
                        crate::sentinel::command::Status::Success => handlers::Status::Completed,
                        crate::sentinel::command::Status::Unconfirmed => {
                            handlers::Status::Unconfirmed
                        }
                        _ => handlers::Status::Failed,
                    };
                    if !matches!(handler.action, Route::Command(_))
                        || result.status != expected
                        || result.execution_id.is_none()
                    {
                        return Err("invalid stored command handler result identity".into());
                    }
                }
                if !results.insert(&result.handler_id)
                    || !declared
                        .iter()
                        .any(|handler| handler.id == result.handler_id)
                    || (result.claim_id.is_none() != result.accepted_at.is_none())
                    || (result.status == handlers::Status::Skipped
                        && (result.claim_id.is_some() || result.execution_id.is_some()))
                    || (result.status != handlers::Status::Skipped && result.claim_id.is_none())
                {
                    return Err("invalid stored handler claim identity".into());
                }
            }
        }
        for handler in job.routes.values().flatten() {
            let expected_kind_id = match &handler.action {
                Route::Sentinel(id) => Some(("blueprint", id)),
                Route::Send(id) => Some(("worker", id)),
                Route::Cron(id) => Some(("cron", id)),
                Route::Notify
                | Route::Stop
                | Route::Command(_)
                | Route::RespondPrompt(_)
                | Route::Escalate(_) => None,
            };
            if let Some((kind, id)) = expected_kind_id {
                Route::parse(&format!(
                    "{}:{id}",
                    if kind == "worker" { "send" } else { kind }
                ))?;
            }
            if let Some(target) = &handler.target {
                if expected_kind_id != Some((target.kind.as_str(), &target.id))
                    || target.workspace_id != job.workspace_id
                    || target.team_id != job.team_id
                    || (target.kind == "blueprint"
                        && target.owner_tab_id.as_deref() != Some(&job.creator_tab_id))
                {
                    return Err("invalid stored exact handler target scope".into());
                }
            }
        }
        if !ids.insert(&job.id)
            || job.creator_tab_id.is_empty()
            || job.workspace_id.is_empty()
            || job.cwd.is_empty()
        {
            return Err("invalid blueprint identity or scope".into());
        }
        if job.probe().len() > 8192
            || (job.pipeline().is_none() && job.question.trim().is_empty())
            || job.question.len() > 4096
        {
            return Err("invalid blueprint probe or question".into());
        }
        if job
            .routes
            .keys()
            .any(|bucket| !job.buckets.contains(bucket))
            || job
                .until
                .as_ref()
                .is_some_and(|bucket| !job.buckets.contains(bucket))
        {
            return Err("invalid blueprint route bucket".into());
        }
    }
    for job in &jobs {
        if let Some(scenario) = &job.scenario_id {
            if !jobs.iter().any(|root| {
                root.id == *scenario
                    && root.scenario_id.as_deref() == Some(root.id.as_str())
                    && root.same_owner(job)
            }) {
                return Err("invalid stored scenario ownership association".into());
            }
        }
    }
    Ok(jobs)
}

impl Store {
    pub fn prepare_pipeline(
        &self,
        spec: codeterm_types::blueprint_pipeline::BlueprintSpec,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let every = match &spec.trigger {
            codeterm_types::blueprint_pipeline::Trigger::Schedule { every } => every.clone(),
            codeterm_types::blueprint_pipeline::Trigger::Event { .. }
            | codeterm_types::blueprint_pipeline::Trigger::Input { .. } => "20s".into(),
        };
        let create = Create {
            terminal: None,
            session: false,
            name: Some(spec.name.clone()),
            probe: None,
            observe: None,
            input: None,
            question: String::new(),
            buckets: Vec::new(),
            every,
            routes: BTreeMap::new(),
            matches: BTreeMap::new(),
            rearm_on_change: BTreeSet::new(),
            until: None,
            timeout: None,
            paused: false,
            handlers: None,
            escalation_timeout_ms: None,
            max_transitions: Some(256),
            max_actions: Some(1024),
        };
        self.prepare_definition(create, owner, connected, Some(spec))
    }

    pub fn prepare(
        &self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        self.prepare_definition(params, owner, connected, None)
    }

    fn prepare_definition(
        &self,
        params: Create,
        owner: Owner,
        connected: bool,
        pipeline: Option<codeterm_types::blueprint_pipeline::BlueprintSpec>,
    ) -> Result<Sentinel, String> {
        if self.admission_closed {
            return Err("blueprint_shutdown_admission_closed".into());
        }
        self.ensure_available()?;
        let Owner {
            creator_tab_id: creator,
            team_id: team,
            workspace_id: workspace,
            cwd,
        } = owner;
        let name = params.name.as_deref().ok_or_else(|| {
            "Blueprint graph name is required; use --name ci-green-watch or --name build-done-notify".to_string()
        })?;
        codeterm_types::automation_name::validate_sentinel_name(name)?;
        let (probe, observe, input) = if pipeline.is_some() {
            (String::new(), None, None)
        } else {
            match (params.probe, params.observe, params.input) {
                (Some(probe), None, None) if !probe.trim().is_empty() && probe.len() <= 8192 => {
                    (probe, None, None)
                }
                (Some(_), None, None) => {
                    return Err("--probe must be nonempty and bounded to 8192 bytes".into());
                }
                (None, Some(binding), None) if source::bounded_id(binding.id()) => {
                    (String::new(), Some(binding), None)
                }
                (None, Some(_), None) => {
                    return Err("--observe: use tab:<id> or cron-run:<id>".into());
                }
                (None, None, Some(input)) => {
                    codeterm_types::sentinel_input::validate(&input)
                        .map_err(|error| format!("--input: {error}"))?;
                    if !params.rearm_on_change.is_empty() {
                        return Err("--rearm-on-change requires a probe source".into());
                    }
                    (String::new(), None, Some(input))
                }
                _ => return Err("provide exactly one of --probe, --observe or --input".into()),
            }
        };
        let interval_ms = duration_ms(&params.every)?;
        if pipeline.is_none() {
            machine::validate(&params.buckets, &params.matches, connected, interval_ms)?;
        }
        if params
            .rearm_on_change
            .iter()
            .any(|bucket| !params.buckets.contains(bucket))
        {
            return Err("--rearm-on-change: name a declared bucket".into());
        }
        if (pipeline.is_none() && params.question.trim().is_empty()) || params.question.len() > 4096
        {
            return Err("--question must be nonempty and bounded to 4096 bytes".into());
        }
        if self.jobs.len() >= MAX_SENTINELS || (!params.paused && self.active_count() >= MAX_ACTIVE)
        {
            return Err("blueprint capacity reached (64 active, 256 total); stop active blueprints or close their Team".into());
        }
        if params
            .until
            .as_ref()
            .is_some_and(|bucket| !params.buckets.contains(bucket))
        {
            return Err("--until: name a declared bucket".into());
        }
        let declared = params.handlers.is_some();
        let (routes, on_other) = if pipeline.is_some() {
            (BTreeMap::new(), Vec::new())
        } else {
            handlers::definitions_with_other(&params.buckets, params.routes, params.handlers)?
        };
        let composition = params.paused
            || declared
            || !on_other.is_empty()
            || routes.values().any(|hs| {
                hs.len() > 1 || hs.iter().any(|h| matches!(h.action, Route::Sentinel(_)))
            });
        if composition && params.timeout.is_none() {
            return Err("composition blueprints require an explicit --timeout".into());
        }
        let max_transitions = params
            .max_transitions
            .unwrap_or(handlers::DEFAULT_TRANSITIONS);
        let max_actions = params.max_actions.unwrap_or(handlers::DEFAULT_ACTIONS);
        if max_transitions == 0 || max_transitions > 256 || max_actions == 0 || max_actions > 1024 {
            return Err(
                "blueprint budgets must be positive, at most 256 transitions and 1024 actions"
                    .into(),
            );
        }
        let now = self.clock.now_ms();
        let expires_at = params
            .timeout
            .as_deref()
            .map(duration_ms)
            .transpose()?
            .map(|duration| now.saturating_add(duration));
        let id = format!(
            "blueprint_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let job = Sentinel {
            source: super::super::stage_source::StageSource::from_parts(
                probe,
                observe,
                input,
                params.terminal,
                pipeline,
            )?,
            session: params.session,
            causal_run: None,
            id: id.clone(),
            team_id: team,
            creator_tab_id: creator,
            workspace_id: workspace,
            cwd,
            question: params.question,
            buckets: params.buckets,
            interval_ms,
            routes,
            matches: params.matches,
            rearm_on_change: params.rearm_on_change,
            until: params.until,
            expires_at,
            next_run: now,
            machine: Machine {
                paused: params.paused,
                ..Machine::default()
            },
            last_reason: None,
            revision: 0,
            agent_authored: false,
            name: params.name,
            scenario_id: if params.paused { None } else { Some(id) },
            max_transitions,
            max_actions,
            transitions: Vec::new(),
            run_transitions: 0,
            run: None,
            on_other,
            escalation_timeout_ms: params.escalation_timeout_ms,
            definition_version: amend::initial_version(),
            amendments: Vec::new(),
        };
        source::validate(&job)?;
        escalation::validate(&job)?;
        Ok(job)
    }
}
