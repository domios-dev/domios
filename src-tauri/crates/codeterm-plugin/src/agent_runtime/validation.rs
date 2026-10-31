use super::*;
use serde::Serialize;
use serde_json::Value;

pub fn validate_runtime_size(value: &impl Serialize, limit: usize) -> Result<(), RuntimeFailure> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.used = self.used.saturating_add(bytes.len());
            if self.used > self.limit {
                return Err(std::io::Error::other("runtime size limit"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter { used: 0, limit }, value).map_err(|_| {
        RuntimeFailure::new(
            RuntimeFailureCode::SizeLimit,
            "runtime value exceeds size limit",
        )
    })
}

fn invalid(message: &str) -> RuntimeFailure {
    RuntimeFailure::new(RuntimeFailureCode::InvalidSchema, message)
}
fn mismatch(message: &str) -> RuntimeFailure {
    RuntimeFailure::new(RuntimeFailureCode::BindingMismatch, message)
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.contains('\0')
}
fn rpc_id(id: &RpcId) -> bool {
    match id {
        RpcId::String(s) => identifier(s),
        RpcId::Number(n) => n.unsigned_abs() <= 9_007_199_254_740_991,
    }
}

pub fn validate_runtime_settings(settings: &RuntimeSettings) -> Result<(), RuntimeFailure> {
    validate_runtime_size(settings, MAX_RUNTIME_VALUE_BYTES)?;
    let mut seen = std::collections::HashSet::new();
    if settings.additional_directories.len() > 32
        || settings.additional_directories.iter().any(|directory| {
            !identifier(directory)
                || !std::path::Path::new(directory).is_absolute()
                || !seen.insert(directory)
        })
    {
        return Err(invalid("invalid additional runtime directories"));
    }
    Ok(())
}

pub fn validate_runtime_context(ctx: &RuntimeContext) -> Result<(), RuntimeFailure> {
    validate_runtime_size(ctx, MAX_RUNTIME_VALUE_BYTES)?;
    if ctx.epoch == 0
        || ctx.epoch > 9_007_199_254_740_991
        || !identifier(&ctx.tab_id)
        || !identifier(&ctx.binding_nonce)
        || !identifier(&ctx.cwd)
        || !identifier(&ctx.host_version)
        || ctx.thread_id.as_deref().is_some_and(|s| !identifier(s))
        || ctx
            .active_turn_id
            .as_deref()
            .is_some_and(|s| !identifier(s))
        || ctx.claimed_sends.len() > MAX_RUNTIME_BATCH
    {
        return Err(invalid("invalid runtime authority"));
    }
    validate_runtime_settings(&ctx.settings)?;
    if let Some(policy) = &ctx.restricted_cli {
        let hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        };
        if policy.tab_id != ctx.tab_id
            || policy.binding_nonce != ctx.binding_nonce
            || policy.epoch != ctx.epoch
            || policy.cwd != ctx.cwd
            || ctx.settings.mode.as_deref() != Some("workspace-write")
            || ctx.settings.network_access != Some(false)
            || !identifier(&policy.executable)
            || !policy
                .executable
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/\\:._-".contains(&byte))
            || !(policy.executable.starts_with('/')
                || std::path::Path::new(&policy.executable).is_absolute())
            || !hash(&policy.executable_sha256)
            || !hash(&policy.config_sha256)
            || !hash(&policy.rule_sha256)
        {
            return Err(mismatch(
                "restricted CLI exception is outside runtime authority",
            ));
        }
    }
    validate_active_turns(ctx)?;
    let mut sends = std::collections::HashSet::new();
    for claim in &ctx.claimed_sends {
        if !identifier(&claim.send_id)
            || ctx.thread_id.as_ref() != Some(&claim.thread_id)
            || !sends.insert(&claim.send_id)
        {
            return Err(mismatch("claimed send is outside runtime thread"));
        }
    }
    if ctx
        .pending_request
        .as_ref()
        .is_some_and(|p| !rpc_id(&p.request_id) || !identifier(&p.method))
    {
        return Err(invalid("invalid pending request"));
    }
    Ok(())
}

pub fn validate_runtime_operation(
    ctx: &RuntimeContext,
    op: &RuntimeOperation,
) -> Result<(), RuntimeFailure> {
    validate_runtime_context(ctx)?;
    validate_runtime_size(op, MAX_RUNTIME_VALUE_BYTES)?;
    match op {
        RuntimeOperation::OpenThread { settings, .. }
        | RuntimeOperation::ResumeThread { settings, .. }
        | RuntimeOperation::SubmitTurn { settings, .. } => validate_runtime_settings(settings)?,
        _ => {}
    }
    if let RuntimeOperation::ForkThread {
        source_thread_id,
        turn_id,
    } = op
    {
        if ctx.thread_id.is_some()
            || ctx.active_turn_id.is_some()
            || !ctx.active_turns.is_empty()
            || !identifier(source_thread_id)
            || !identifier(turn_id)
        {
            return Err(mismatch(
                "fork requires an unbound runtime and valid source boundary",
            ));
        }
    }
    if let RuntimeOperation::BackgroundProcesses { thread_id, action } = op {
        if !owns_runtime_thread(ctx, thread_id) {
            return Err(mismatch("background operation targets an unowned thread"));
        }
        match action {
            RuntimeBackgroundProcessAction::Terminate { process_id } if !identifier(process_id) => {
                return Err(invalid("invalid background process id"));
            }
            RuntimeBackgroundProcessAction::List {
                cursor: Some(cursor),
            } if !identifier(cursor) => return Err(invalid("invalid background process cursor")),
            _ => {}
        }
    }
    let thread = match op {
        RuntimeOperation::ResumeThread { thread_id, .. } => {
            if ctx
                .thread_id
                .as_ref()
                .is_some_and(|bound| bound != thread_id)
            {
                return Err(mismatch("resume retargets thread"));
            }
            if !identifier(thread_id) {
                return Err(invalid("empty resume target"));
            }
            None
        }
        RuntimeOperation::ForkThread { .. } => None,
        RuntimeOperation::AnswerQuestion { thread_id, .. }
        | RuntimeOperation::ReadHistory { thread_id, .. }
        | RuntimeOperation::SubmitTurn { thread_id, .. }
        | RuntimeOperation::Interrupt { thread_id, .. } => Some(thread_id),
        RuntimeOperation::Control { thread_id, .. } => thread_id.as_ref(),
        RuntimeOperation::Answer { request_id, .. } => {
            if !ctx
                .pending_request
                .as_ref()
                .is_some_and(|p| &p.request_id == request_id)
            {
                return Err(mismatch("answer targets unknown request"));
            }
            None
        }
        RuntimeOperation::OpenThread { .. } if ctx.thread_id.is_some() => {
            return Err(mismatch("open replaces bound thread"));
        }
        _ => None,
    };
    if thread.is_some_and(|thread| {
        ctx.thread_id.as_ref() != Some(thread)
            && !(matches!(op, RuntimeOperation::ReadHistory { .. })
                && runtime_descendant(ctx, thread))
    }) {
        return Err(mismatch("operation retargets thread"));
    }
    match op {
        RuntimeOperation::AnswerQuestion {
            item_id, answers, ..
        } => {
            if !identifier(item_id) || answers.is_empty() || answers.len() > 32 {
                return Err(invalid("invalid question answer"));
            }
            let mut ids = std::collections::HashSet::new();
            for answer in answers {
                if !identifier(&answer.question_id)
                    || !ids.insert(&answer.question_id)
                    || answer.values.len() > 128
                    || answer.values.iter().any(|value| !identifier(value))
                    || answer.text.as_ref().is_some_and(|text| text.len() > 65536)
                {
                    return Err(invalid("invalid question answer values"));
                }
            }
        }
        RuntimeOperation::SubmitTurn {
            send_id,
            active_turn_id,
            ..
        } => {
            if !identifier(send_id) {
                return Err(invalid("invalid send id"));
            }
            if active_turn_id != &ctx.active_turn_id {
                return Err(mismatch("submit retargets turn"));
            }
        }
        RuntimeOperation::Interrupt { active_turn_id, .. }
            if active_turn_id != &ctx.active_turn_id =>
        {
            return Err(mismatch("interrupt retargets turn"));
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_runtime_plan(
    cap: &AgentRuntimeCapability,
    ctx: &RuntimeContext,
    op: &RuntimeOperation,
    plan: &RuntimePlan,
) -> Result<(), RuntimeFailure> {
    validate_runtime_operation(ctx, op)?;
    validate_runtime_size(plan, MAX_RUNTIME_VALUE_BYTES)?;
    validate_runtime_size(&plan.state, MAX_RUNTIME_STATE_BYTES)?;
    if matches!(op, RuntimeOperation::ForkThread { .. })
        && !cap
            .thread_history
            .as_ref()
            .is_some_and(|history| history.actions.contains(&RuntimeThreadHistoryAction::Fork))
    {
        return Err(mismatch("fork thread history capability absent"));
    }
    if let RuntimeOperation::ReadHistory { thread_id, .. } = op {
        if runtime_descendant(ctx, thread_id)
            && !super::children::child_history_request(&plan.action, thread_id)
        {
            return Err(mismatch("descendant history must be an exact read request"));
        }
    }
    if matches!(op, RuntimeOperation::AnswerQuestion { .. })
        && !matches!(plan.action, RuntimeAction::QuestionAnswer { .. })
    {
        return Err(mismatch("question answer must be local formatting"));
    }
    match &plan.action {
        RuntimeAction::QuestionAnswer { input } => {
            if !matches!(op, RuntimeOperation::AnswerQuestion { .. }) {
                return Err(mismatch("question answer action outside answerQuestion"));
            }
            validate_runtime_size(input, MAX_RUNTIME_VALUE_BYTES)
        }
        RuntimeAction::Request { call } => validate_runtime_call(cap, ctx, op, call),
        RuntimeAction::Notification { notification } => {
            validate_runtime_call(cap, ctx, op, notification)
        }
        RuntimeAction::Reply { request_id, result } => {
            if !matches!(op, RuntimeOperation::Answer { request_id: target, .. } if target == request_id)
                || !ctx
                    .pending_request
                    .as_ref()
                    .is_some_and(|p| &p.request_id == request_id)
            {
                return Err(mismatch("reply targets foreign server request"));
            }
            validate_runtime_reply(result)
        }
    }
}

pub fn validate_runtime_reply(reply: &RpcReply) -> Result<(), RuntimeFailure> {
    validate_runtime_size(reply, MAX_RUNTIME_VALUE_BYTES)?;
    if let RpcReply::Error { error } = reply {
        if error.code.unsigned_abs() > 9_007_199_254_740_991 {
            return Err(invalid("unsafe RPC error code"));
        }
    }
    Ok(())
}

pub fn validate_runtime_call(
    cap: &AgentRuntimeCapability,
    ctx: &RuntimeContext,
    op: &RuntimeOperation,
    call: &RpcCall,
) -> Result<(), RuntimeFailure> {
    if let RuntimeOperation::ForkThread {
        source_thread_id,
        turn_id,
    } = op
    {
        let params = &call.params;
        let valid = call.target.is_none()
            && match call.method.as_str() {
                "thread/turns/list" => {
                    params["threadId"].as_str() == Some(source_thread_id)
                        && params["itemsView"] == "notLoaded"
                }
                "thread/fork" => {
                    params["threadId"].as_str() == Some(source_thread_id)
                        && params["lastTurnId"].as_str() == Some(turn_id)
                }
                _ => false,
            };
        if !valid {
            return Err(mismatch("fork request differs from its source boundary"));
        }
    }
    if let RuntimeOperation::BackgroundProcesses { thread_id, .. } = op {
        if !cap.background_processes {
            return Err(mismatch("background processes capability absent"));
        }
        if call
            .target
            .as_ref()
            .is_none_or(|target| &target.thread_id != thread_id || target.turn_id.is_some())
        {
            return Err(mismatch("background request lacks exact owned target"));
        }
    } else if matches!(op, RuntimeOperation::Interrupt { .. }) {
        let target = call
            .target
            .as_ref()
            .ok_or_else(|| mismatch("interrupt request lacks target"))?;
        let active = ctx.active_turns.iter().any(|turn| {
            turn.thread_id == target.thread_id && target.turn_id.as_ref() == Some(&turn.turn_id)
        });
        if !active {
            return Err(mismatch("interrupt targets an inactive or unowned turn"));
        }
    } else if let Some(target) = &call.target {
        let operation_target = match op {
            RuntimeOperation::ReadHistory { thread_id, .. } => Some(thread_id),
            _ => ctx.thread_id.as_ref(),
        };
        if operation_target != Some(&target.thread_id)
            || target
                .turn_id
                .as_ref()
                .is_some_and(|id| ctx.active_turn_id.as_ref() != Some(id))
        {
            return Err(mismatch("call target is outside operation authority"));
        }
    }
    if !cap.permits_method(op, &call.method) {
        return Err(RuntimeFailure::new(
            RuntimeFailureCode::MethodNotAllowed,
            "method is not declared for operation",
        ));
    }
    validate_runtime_size(call, MAX_RUNTIME_VALUE_BYTES)
}

pub fn validate_runtime_step(
    cap: &AgentRuntimeCapability,
    ctx: &RuntimeContext,
    op: &RuntimeOperation,
    reply: &RpcReply,
    step: &RuntimeStep,
) -> Result<(), RuntimeFailure> {
    validate_runtime_size(step, MAX_RUNTIME_VALUE_BYTES)?;
    match step {
        RuntimeStep::Continue {
            history,
            plan,
            before_acceptance,
        } => {
            if matches!(op, RuntimeOperation::SubmitTurn { .. })
                && (!before_acceptance || !matches!(reply, RpcReply::Error { .. }))
            {
                return Err(mismatch("accepted or uncertain submit cannot continue"));
            }
            if let Some(history) = history {
                if !matches!(
                    history,
                    RuntimeResult::HistoryPage {
                        next_cursor: None,
                        ..
                    }
                ) {
                    return Err(mismatch("continuation requires a history chunk"));
                }
                validate_runtime_result(ctx, op, history)?;
            }
            validate_runtime_plan(cap, ctx, op, plan)
        }
        RuntimeStep::Complete {
            result,
            notification,
            events,
        } => {
            if matches!(op, RuntimeOperation::ReadHistory { thread_id, .. } if runtime_descendant(ctx, thread_id))
                && (notification.is_some()
                    || events.as_ref().is_some_and(|events| !events.is_empty()))
            {
                return Err(mismatch("descendant history cannot publish side effects"));
            }
            if let Some(events) = events {
                if events
                    .iter()
                    .any(|event| matches!(event, AgentRuntimeEvent::SendEvidence { .. }))
                {
                    return Err(mismatch("correlated response cannot confirm delivery"));
                }
                validate_runtime_reduction(
                    ctx,
                    &RuntimeReduction {
                        state: Value::Null,
                        events: events.clone(),
                    },
                )?;
            }
            if let Some(call) = notification {
                validate_runtime_call(cap, ctx, op, call)?;
            }
            validate_runtime_result(ctx, op, result)
        }
        RuntimeStep::Refused {
            before_acceptance, ..
        } => {
            if *before_acceptance
                && matches!(op, RuntimeOperation::SubmitTurn { .. })
                && !matches!(reply, RpcReply::Error { .. })
            {
                return Err(mismatch("success cannot be pre-acceptance refusal"));
            }
            Ok(())
        }
    }
}

fn validate_runtime_result(
    ctx: &RuntimeContext,
    op: &RuntimeOperation,
    result: &RuntimeResult,
) -> Result<(), RuntimeFailure> {
    if let RuntimeResult::ThreadLoaded { settings, .. } = result {
        validate_runtime_settings(settings)?;
    }
    let valid = match (op, result) {
        (
            RuntimeOperation::ForkThread {
                source_thread_id: source,
                turn_id: expected_turn,
            },
            RuntimeResult::ThreadForked {
                source_thread_id,
                thread_id,
                turn_id,
            },
        ) => {
            source == source_thread_id
                && expected_turn == turn_id
                && identifier(thread_id)
                && identifier(turn_id)
                && thread_id != source
        }
        (
            RuntimeOperation::Control {
                thread_id: Some(target),
                action,
                ..
            },
            RuntimeResult::Goal { thread_id, .. },
        ) => action == "goal" && target == thread_id,
        (
            RuntimeOperation::BackgroundProcesses {
                thread_id: target,
                action: RuntimeBackgroundProcessAction::List { cursor },
            },
            RuntimeResult::BackgroundProcesses {
                thread_id,
                processes,
                next_cursor,
            },
        ) => {
            validate_background_processes(processes)?;
            target == thread_id
                && next_cursor
                    .as_ref()
                    .is_none_or(|next| identifier(next) && Some(next) != cursor.as_ref())
        }
        (
            RuntimeOperation::BackgroundProcesses {
                thread_id: target,
                action:
                    RuntimeBackgroundProcessAction::Terminate {
                        process_id: target_process,
                    },
            },
            RuntimeResult::BackgroundProcessTermination {
                thread_id,
                process_id,
                ..
            },
        ) => target == thread_id && target_process == process_id,
        (
            RuntimeOperation::BackgroundProcesses {
                action: RuntimeBackgroundProcessAction::Clean,
                ..
            },
            RuntimeResult::Done,
        ) => true,
        (RuntimeOperation::Initialize, RuntimeResult::Initialized { .. }) => true,
        (RuntimeOperation::OpenThread { .. }, RuntimeResult::ThreadLoaded { thread_id, .. }) => {
            ctx.thread_id.is_none() && identifier(thread_id)
        }
        (
            RuntimeOperation::ResumeThread {
                thread_id: target, ..
            },
            RuntimeResult::ThreadLoaded { thread_id, .. },
        ) => target == thread_id,
        (
            RuntimeOperation::ReadHistory {
                thread_id: target,
                order: requested,
                ..
            },
            RuntimeResult::HistoryPage {
                thread_id,
                items,
                open_items,
                questions,
                order: echoed,
                ..
            },
        ) => {
            if echoed.is_some() && echoed != requested {
                return Err(mismatch("history order echo differs from the request"));
            }
            if let Some(questions) = questions {
                if questions.len() > MAX_RUNTIME_BATCH {
                    return Err(invalid("too many history questions"));
                }
                let mut ids = std::collections::HashSet::new();
                for question in questions {
                    if &question.thread_id != thread_id || !ids.insert(&question.item_id) {
                        return Err(mismatch(
                            "history question retargets thread or duplicates item",
                        ));
                    }
                    validate_question_fields(&question.item_id, &question.questions)?;
                }
            }
            target == thread_id
                && items.len() <= MAX_RUNTIME_BATCH
                && open_items.len() <= MAX_RUNTIME_BATCH
                && items.iter().all(|item| identifier(&item.id))
                && open_items
                    .iter()
                    .all(|item| identifier(&item.turn_id) && identifier(&item.item.id))
        }
        (
            RuntimeOperation::SubmitTurn {
                thread_id: target,
                send_id: send,
                ..
            },
            RuntimeResult::TurnAccepted {
                thread_id, send_id, ..
            },
        ) => target == thread_id && send == send_id,
        (
            RuntimeOperation::Interrupt { .. }
            | RuntimeOperation::Answer { .. }
            | RuntimeOperation::Control { .. },
            RuntimeResult::Done,
        ) => true,
        (RuntimeOperation::Control { action, .. }, RuntimeResult::ResetCreditConsumed { .. }) => {
            action == "redeemResetCredit"
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(mismatch("result does not match operation authority"))
    }
}

pub fn validate_runtime_frames(
    cap: &AgentRuntimeCapability,
    frames: &[RuntimeFrame],
) -> Result<(), RuntimeFailure> {
    validate_runtime_size(&frames, MAX_RUNTIME_VALUE_BYTES)?;
    if frames.len() > MAX_RUNTIME_BATCH {
        return Err(RuntimeFailure::new(
            RuntimeFailureCode::SizeLimit,
            "too many runtime frames",
        ));
    }
    for frame in frames {
        match frame {
            RuntimeFrame::Notification { method, .. }
                if !cap.notification_methods.contains(method) =>
            {
                return Err(RuntimeFailure::new(
                    RuntimeFailureCode::MethodNotAllowed,
                    "undeclared inbound notification",
                ));
            }
            RuntimeFrame::Request {
                request_id, method, ..
            } if !rpc_id(request_id) || !identifier(method) => {
                return Err(invalid("invalid server request"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn validate_runtime_reduction(
    ctx: &RuntimeContext,
    reduction: &RuntimeReduction,
) -> Result<(), RuntimeFailure> {
    validate_runtime_size(reduction, MAX_RUNTIME_VALUE_BYTES)?;
    validate_runtime_size(&reduction.state, MAX_RUNTIME_STATE_BYTES)?;
    if reduction.events.len() > MAX_RUNTIME_BATCH {
        return Err(RuntimeFailure::new(
            RuntimeFailureCode::SizeLimit,
            "too many runtime events",
        ));
    }
    validate_active_turns(ctx)?;
    let mut parents: std::collections::HashMap<String, Option<String>> = ctx
        .thread_parents
        .iter()
        .map(|thread| {
            (
                thread.thread_id.clone(),
                Some(thread.parent_thread_id.clone()),
            )
        })
        .collect();
    if let Some(root) = &ctx.thread_id {
        parents.insert(root.clone(), None);
    }
    let mut active: std::collections::HashSet<String> = ctx
        .active_turns
        .iter()
        .map(|turn| turn.thread_id.clone())
        .collect();
    for event in &reduction.events {
        if let AgentRuntimeEvent::BackgroundProcesses {
            thread_id,
            processes,
        } = event
        {
            if !owns_runtime_thread(ctx, thread_id) {
                return Err(mismatch("background event targets an unowned thread"));
            }
            validate_background_processes(processes)?;
            continue;
        }
        if let AgentRuntimeEvent::AuthLogin { login } = event {
            if login
                .expires_at_ms
                .is_some_and(|expiry| expiry > 9_007_199_254_740_991)
                || login.login_id.as_ref().is_some_and(|id| !identifier(id))
                || login
                    .user_code
                    .as_ref()
                    .is_some_and(|code| !identifier(code))
                || login
                    .error
                    .as_ref()
                    .is_some_and(|error| error.len() > 65536)
                || login.url.as_ref().is_some_and(|url| {
                    url.len() > 16384
                        || !url.starts_with("https://")
                        || url.contains(['\r', '\n', '\0'])
                })
            {
                return Err(invalid("invalid login event"));
            }
            continue;
        }
        if let AgentRuntimeEvent::Auth { detail, .. } = event {
            if detail.as_ref().is_some_and(|s| s.len() > 65536) {
                return Err(invalid("auth detail exceeds bound"));
            }
            continue;
        }
        if let AgentRuntimeEvent::TurnState {
            thread_id,
            parent_thread_id,
            turn_id,
            state,
            detail,
            status_line,
            ..
        } = event
        {
            if !identifier(thread_id) || turn_id.as_ref().is_some_and(|id| !identifier(id)) {
                return Err(invalid("invalid turn state identifiers"));
            }
            if detail.is_some_and(|detail| detail.state() != *state) {
                return Err(invalid("turn detail does not match its state"));
            }
            if status_line.as_ref().is_some_and(|line| line.len() > 4096) {
                return Err(invalid("turn status line exceeds bound"));
            }
            if ctx.thread_id.as_ref() == Some(thread_id) {
                if parent_thread_id.is_some() {
                    return Err(mismatch("root turn event has parent"));
                }
            } else if let Some(known_parent) = parents.get(thread_id) {
                if parent_thread_id
                    .as_ref()
                    .is_some_and(|parent| known_parent.as_ref() != Some(parent))
                {
                    return Err(mismatch("descendant changes proven parent"));
                }
            } else {
                let parent = parent_thread_id
                    .as_ref()
                    .ok_or_else(|| mismatch("unverified descendant turn"))?;
                if parent == thread_id || !parents.contains_key(parent) {
                    return Err(mismatch("unverified descendant turn"));
                }
                if parents.len() >= 65 {
                    return Err(invalid("too many known thread parents"));
                }
                parents.insert(thread_id.clone(), Some(parent.clone()));
            }
            if matches!(state, RuntimeTurnState::Idle) {
                active.remove(thread_id);
            } else {
                active.insert(thread_id.clone());
            }
            if active.len() > 32 {
                return Err(invalid("too many active turns"));
            }
            continue;
        }
        if let AgentRuntimeEvent::RateLimits {
            thread_id,
            rate_limits,
            resets,
            ..
        } = event
        {
            if thread_id
                .as_ref()
                .is_some_and(|thread| ctx.thread_id.as_ref() != Some(thread))
            {
                return Err(mismatch("rate limits retarget thread"));
            }
            validate_rate_limits(rate_limits)?;
            if let Some(resets) = resets {
                if resets.credits.as_ref().is_some_and(|credits| {
                    credits.len() > 256 || credits.iter().any(|credit| !identifier(&credit.id))
                }) {
                    return Err(invalid("invalid reset credit details"));
                }
            }
            continue;
        }
        let thread = match event {
            AgentRuntimeEvent::BackgroundProcesses { .. }
            | AgentRuntimeEvent::RateLimits { .. }
            | AgentRuntimeEvent::Auth { .. }
            | AgentRuntimeEvent::AuthLogin { .. } => continue,
            AgentRuntimeEvent::ChatItem { thread_id, item } => {
                if !identifier(&item.id) {
                    return Err(invalid("invalid chat item id"));
                }
                if let Some(child) = runtime_child(thread_id, item) {
                    if !parents.contains_key(&child.parent_thread_id)
                        || parents
                            .get(&child.thread_id)
                            .is_some_and(|parent| parent.as_ref() != Some(&child.parent_thread_id))
                        || (!parents.contains_key(&child.thread_id) && parents.len() >= 65)
                    {
                        return Err(mismatch("child metadata changes proven ancestry"));
                    }
                    parents.insert(child.thread_id, Some(child.parent_thread_id));
                }
                thread_id
            }
            AgentRuntimeEvent::TurnState { thread_id, .. }
            | AgentRuntimeEvent::Usage { thread_id, .. }
            | AgentRuntimeEvent::Prompt { thread_id, .. }
            | AgentRuntimeEvent::PromptResolved { thread_id, .. }
            | AgentRuntimeEvent::ControlsConfirmed { thread_id, .. }
            | AgentRuntimeEvent::Question { thread_id, .. }
            | AgentRuntimeEvent::ContextCompacted { thread_id, .. }
            | AgentRuntimeEvent::ThreadReverted { thread_id } => thread_id,
            AgentRuntimeEvent::SendEvidence {
                item_id,
                thread_id,
                send_id,
                turn_id,
            } => {
                if !identifier(turn_id)
                    || item_id.as_ref().is_some_and(|id| !identifier(id))
                    || !ctx
                        .claimed_sends
                        .iter()
                        .any(|claim| &claim.send_id == send_id && &claim.thread_id == thread_id)
                {
                    return Err(mismatch("send evidence has no live claim"));
                }
                thread_id
            }
        };
        if ctx.thread_id.as_ref() != Some(thread) {
            return Err(mismatch("event retargets runtime thread"));
        }
        match event {
            AgentRuntimeEvent::ControlsConfirmed { settings, .. } => {
                validate_runtime_settings(settings)?
            }
            AgentRuntimeEvent::Question {
                item_id, questions, ..
            } => {
                validate_question_fields(item_id, questions)?;
            }
            AgentRuntimeEvent::Prompt { request_id, .. }
            | AgentRuntimeEvent::PromptResolved { request_id, .. }
                if !rpc_id(request_id) =>
            {
                return Err(invalid("invalid prompt request id"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_question_fields(
    item_id: &str,
    questions: &[RuntimeQuestion],
) -> Result<(), RuntimeFailure> {
    if !identifier(item_id) || questions.is_empty() || questions.len() > 32 {
        return Err(invalid("invalid question event"));
    }
    let mut ids = std::collections::HashSet::new();
    for question in questions {
        if !identifier(&question.id)
            || !ids.insert(&question.id)
            || question.text.len() > 65536
            || question.options.len() > 128
        {
            return Err(invalid("invalid question fields"));
        }
        let mut options = std::collections::HashSet::new();
        if question.options.iter().any(|option| {
            !identifier(&option.value)
                || !options.insert(&option.value)
                || option.label.len() > 65536
        }) {
            return Err(invalid("invalid question options"));
        }
    }
    Ok(())
}

pub fn validate_runtime_state(state: &Value) -> Result<(), RuntimeFailure> {
    validate_runtime_size(state, MAX_RUNTIME_STATE_BYTES)
}

pub fn validate_host_runtime_event(
    ctx: &RuntimeContext,
    event: &HostAgentRuntimeEvent,
) -> Result<(), RuntimeFailure> {
    validate_host_runtime_events(ctx, std::slice::from_ref(event))
}

pub fn validate_host_runtime_events(
    ctx: &RuntimeContext,
    events: &[HostAgentRuntimeEvent],
) -> Result<(), RuntimeFailure> {
    validate_runtime_size(&events, MAX_RUNTIME_VALUE_BYTES)?;
    if events.len() > MAX_RUNTIME_BATCH {
        return Err(invalid("too many host runtime events"));
    }
    for event in events {
        validate_host_timestamp(event)?;
    }
    validate_runtime_reduction(
        ctx,
        &RuntimeReduction {
            state: Value::Null,
            events: events.iter().map(|event| event.event.clone()).collect(),
        },
    )
}

fn validate_host_timestamp(event: &HostAgentRuntimeEvent) -> Result<(), RuntimeFailure> {
    if event.at == 0 || event.at > 9_007_199_254_740_991 {
        return Err(invalid("invalid host receipt timestamp"));
    }
    if let AgentRuntimeEvent::TurnState {
        started_at,
        completed_at,
        ..
    } = &event.event
    {
        if started_at
            .iter()
            .chain(completed_at.iter())
            .any(|stamp| *stamp > 9_007_199_254_740_991)
            || started_at
                .zip(*completed_at)
                .is_some_and(|(start, end)| end < start)
        {
            return Err(invalid("invalid provider turn timestamp"));
        }
    }
    Ok(())
}

pub fn validate_rate_limits(limits: &[RuntimeRateLimit]) -> Result<(), RuntimeFailure> {
    if limits.len() > 4
        || limits.iter().any(|limit| {
            !identifier(&limit.window)
                || !limit.used_percent.is_finite()
                || !(0.0..=100.0).contains(&limit.used_percent)
                || limit.resets_at.is_some_and(|at| at > 9_007_199_254_740_991)
                || limit
                    .window_minutes
                    .is_some_and(|minutes| minutes == 0 || minutes > 9_007_199_254_740_991)
        })
    {
        return Err(invalid("invalid rate limits"));
    }
    Ok(())
}

pub fn validate_thread_parents(ctx: &RuntimeContext) -> Result<(), RuntimeFailure> {
    if ctx.thread_parents.len() > 64 {
        return Err(invalid("too many thread parents"));
    }
    let mut known = std::collections::HashMap::new();
    for thread in &ctx.thread_parents {
        if !identifier(&thread.thread_id)
            || !identifier(&thread.parent_thread_id)
            || ctx.thread_id.as_ref() == Some(&thread.thread_id)
            || known
                .insert(&thread.thread_id, &thread.parent_thread_id)
                .is_some()
        {
            return Err(invalid("invalid thread ancestry"));
        }
    }
    for thread in &ctx.thread_parents {
        let mut current = &thread.thread_id;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current) {
                return Err(mismatch("thread ancestry cycle"));
            }
            let parent = known
                .get(current)
                .ok_or_else(|| mismatch("thread has unknown parent"))?;
            if ctx.thread_id.as_ref() == Some(parent) {
                break;
            }
            current = parent;
        }
    }
    Ok(())
}

pub fn validate_active_turns(ctx: &RuntimeContext) -> Result<(), RuntimeFailure> {
    validate_thread_parents(ctx)?;
    if ctx.active_turns.len() > 32 {
        return Err(invalid("too many active turns"));
    }
    let mut threads = std::collections::HashSet::new();
    for turn in &ctx.active_turns {
        if !identifier(&turn.thread_id)
            || !identifier(&turn.turn_id)
            || !threads.insert(&turn.thread_id)
        {
            return Err(invalid("invalid active turn roster"));
        }
        if ctx.thread_id.as_ref() == Some(&turn.thread_id) {
            if turn.parent_thread_id.is_some() {
                return Err(mismatch("root turn has a parent"));
            }
        } else {
            let parent = ctx
                .thread_parents
                .iter()
                .find(|thread| thread.thread_id == turn.thread_id)
                .ok_or_else(|| mismatch("active descendant has no known ancestry"))?;
            if turn
                .parent_thread_id
                .as_ref()
                .is_some_and(|source| source != &parent.parent_thread_id)
            {
                return Err(mismatch("active descendant changes proven parent"));
            }
        }
    }
    Ok(())
}
