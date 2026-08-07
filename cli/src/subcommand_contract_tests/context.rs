//! Caller-context coverage. Owned by the context lane;
//! pre-created here so two live lanes never edit the shared test root.

use serde_json::{json, Value};

use super::{env_lock, restore_env};
use crate::cmd::plan::caller_context::CallerContext;
use crate::cmd::plan::{
    caller_tab_id, inject_trusted_session, maybe_inject_caller_tab, owner_from_env,
};

const TAB: &str = "CODETERM_TAB_ID";
const SESSION: &str = "CODETERM_SESSION_ID";
const NONCE: &str = "CODETERM_SESSION_BINDING_NONCE";
const TITLE: &str = "CODETERM_TAB_TITLE";
const CONTEXT_ENV: [&str; 4] = [TAB, SESSION, NONCE, TITLE];

/// Runs `body` with exactly `set` present in the environment, restoring every
/// context variable afterwards so sibling tests observe the original process env.
fn with_env<T>(set: &[(&str, &str)], body: impl FnOnce() -> T) -> T {
    let guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let saved: Vec<(&str, Option<String>)> = CONTEXT_ENV
        .iter()
        .map(|key| (*key, std::env::var(key).ok()))
        .collect();
    for key in CONTEXT_ENV {
        std::env::remove_var(key);
    }
    for (key, value) in set {
        std::env::set_var(key, value);
    }
    let out = body();
    for (key, value) in saved {
        restore_env(key, value);
    }
    drop(guard);
    out
}

fn full_env() -> Vec<(&'static str, &'static str)> {
    vec![
        (TAB, "tab_ctx"),
        (SESSION, "sess_ctx"),
        (NONCE, "nonce_ctx"),
        (TITLE, "Context Tab"),
    ]
}

// Frozen wire shapes captured before the first edit; every row below asserts
// pre/post equality against them.

fn frozen_owner() -> Value {
    json!({ "tab_id": "tab_ctx", "session_id": "sess_ctx", "title": "Context Tab" })
}

fn frozen_owner_absent() -> Value {
    json!({ "tab_id": null, "session_id": null, "title": null })
}

fn frozen_caller_tab() -> Value {
    json!({ "seed": 1, "caller_tab_id": "tab_ctx" })
}

fn frozen_trusted() -> Value {
    json!({
        "seed": 1,
        "caller_tab_id": "tab_ctx",
        "session_id": "sess_ctx",
        "session_binding_nonce": "nonce_ctx",
    })
}

#[test]
fn caller_context_round_trips_supplied_values() {
    let (owner, tab) = with_env(&full_env(), || {
        let ctx = CallerContext::from_env();
        (ctx.owner(), ctx.tab_id())
    });
    assert_eq!(owner, frozen_owner());
    assert_eq!(tab.as_deref(), Some("tab_ctx"));
}

#[test]
fn context_injection_matches_pre_change_params() {
    let (caller_tab, trusted, owner) = with_env(&full_env(), || {
        let ctx = CallerContext::from_env();
        let mut caller_tab = json!({ "seed": 1 });
        ctx.apply_caller_tab(&mut caller_tab);
        let mut trusted = json!({ "seed": 1 });
        ctx.apply_trusted_session(&mut trusted);
        (caller_tab, trusted, ctx.owner())
    });
    assert_eq!(caller_tab, frozen_caller_tab());
    assert_eq!(trusted, frozen_trusted());
    assert_eq!(owner, frozen_owner());
}

/// The `from`/`owner` object is preserved legacy shape: all three keys stay
/// present as JSON null, because dropping them could flip a daemon has_key check.
#[test]
fn env_absent_yields_absent_context() {
    let (caller_tab, trusted, owner) = with_env(&[], || {
        let ctx = CallerContext::from_env();
        let mut caller_tab = json!({ "seed": 1 });
        ctx.apply_caller_tab(&mut caller_tab);
        let mut trusted = json!({ "seed": 1 });
        ctx.apply_trusted_session(&mut trusted);
        (caller_tab, trusted, ctx.owner())
    });

    for key in ["caller_tab_id", "session_id", "session_binding_nonce"] {
        assert!(
            caller_tab.get(key).is_none(),
            "{key} must be absent, not null, when its env var is unset"
        );
        assert!(
            trusted.get(key).is_none(),
            "{key} must be absent, not null, when its env var is unset"
        );
    }
    assert_eq!(caller_tab, json!({ "seed": 1 }));
    assert_eq!(trusted, json!({ "seed": 1 }));

    assert_eq!(owner, frozen_owner_absent());
    for key in ["tab_id", "session_id", "title"] {
        assert!(
            owner.get(key).is_some_and(Value::is_null),
            "owner.{key} must stay present-and-null"
        );
    }
}

#[test]
fn trusted_session_scope_is_unchanged() {
    let partial = with_env(&[(TAB, "tab_ctx"), (NONCE, "nonce_ctx")], || {
        let mut params = json!({ "seed": 1 });
        CallerContext::from_env().apply_trusted_session(&mut params);
        params
    });
    assert_eq!(
        partial,
        json!({ "seed": 1, "caller_tab_id": "tab_ctx", "session_binding_nonce": "nonce_ctx" }),
        "each trusted field is injected only when its own env var exists"
    );

    let non_object = with_env(&full_env(), || {
        let mut params = json!("not-an-object");
        CallerContext::from_env().apply_trusted_session(&mut params);
        params
    });
    assert_eq!(non_object, json!("not-an-object"));
}

/// The surviving helpers are adapters; if they ever stop delegating, their
/// output and `CallerContext`'s diverge here.
#[test]
fn legacy_helpers_delegate_to_caller_context() {
    with_env(&full_env(), || {
        let ctx = CallerContext::from_env();

        assert_eq!(owner_from_env(), ctx.owner());
        assert_eq!(caller_tab_id(), ctx.tab_id());

        let mut helper = json!({ "seed": 1 });
        maybe_inject_caller_tab(&mut helper);
        let mut typed = json!({ "seed": 1 });
        ctx.apply_caller_tab(&mut typed);
        assert_eq!(helper, typed);
        assert_eq!(helper, frozen_caller_tab());

        let mut helper = json!({ "seed": 1 });
        inject_trusted_session(&mut helper);
        let mut typed = json!({ "seed": 1 });
        ctx.apply_trusted_session(&mut typed);
        assert_eq!(helper, typed);
        assert_eq!(helper, frozen_trusted());
    });

    with_env(&[], || {
        let ctx = CallerContext::from_env();
        assert_eq!(owner_from_env(), ctx.owner());
        assert_eq!(caller_tab_id(), ctx.tab_id());
    });
}

/// Opening marker of an inline `#[cfg(test)] mod tests` body.
const TEST_MOD_MARKER: &str = "mod tests {";

/// Everything before a file's inline test module. Without this a test that
/// merely mentions a guarded token would change what its guard observes.
fn production(source: &str) -> &str {
    source
        .split_once(TEST_MOD_MARKER)
        .map_or(source, |(before, _)| before)
}

/// Declares every guarded source through `production`, so no site below can
/// reach the raw `include_str!` text.
macro_rules! guarded_sources {
    ($($name:ident => $path:literal,)+) => {
        $(fn $name() -> &'static str { production(include_str!($path)) })+
    };
}

guarded_sources! {
    plan_rs => "../cmd/plan.rs",
    qa_rs => "../cmd/plan/qa_cmd.rs",
    gate_rs => "../cmd/plan/gate_cmd.rs",
    read_rs => "../cmd/plan/read_cmd.rs",
    local_io_rs => "../cmd/local_io.rs",
    orchestrator_rs => "../cmd/orchestrator.rs",
    agent_rs => "../cmd/agent.rs",
    task_rs => "../cmd/task.rs",
    caller_context_rs => "../cmd/plan/caller_context.rs",
}

/// Source text of `signature`'s body, up to the next item at column zero.
fn fn_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let rest = &source[start + signature.len()..];
    let open = rest.find('{').expect("function body must start");
    let mut depth = 0;
    for (offset, byte) in rest[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open..open + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("{signature} body must terminate")
}

fn call_plan_rpc_body() -> &'static str {
    let plan = plan_rs();
    let start = plan
        .find("pub(crate) fn call_plan_rpc(")
        .expect("call_plan_rpc must exist in cmd/plan.rs");
    let rest = &plan[start..];
    let end = rest
        .find("\nfn call(")
        .expect("call_plan_rpc is followed by fn call");
    &rest[..end]
}

/// One row per bounded injection point: (file, source, expected apply calls).
/// Counts are frozen; a command gaining context changes one of them.
fn bounded() -> [(&'static str, &'static str, usize); 4] {
    [
        ("qa_cmd.rs", qa_rs(), 5),
        ("gate_cmd.rs", gate_rs(), 3),
        ("read_cmd.rs", read_rs(), 1),
        ("local_io.rs", local_io_rs(), 1),
    ]
}

/// A guard must observe production text only, so an inline `cfg(test)` body can
/// neither satisfy nor break it.
#[test]
fn guards_observe_production_text_only() {
    const TOKEN: &str = "ctx.owner()";

    let test_only = concat!(
        "fn ship(params: &mut Value) {\n    params[\"seed\"] = json!(1);\n}\n\n",
        "#[cfg(test)]\nmod tests {\n    fn t(ctx: &CallerContext) {\n",
        "        let _ = ctx.owner();\n    }\n}\n",
    );
    assert_eq!(
        test_only.matches(TOKEN).count(),
        1,
        "fixture must carry the token, otherwise the exclusion proves nothing"
    );
    let scoped = production(test_only);
    assert_eq!(
        scoped.matches(TOKEN).count(),
        0,
        "a token that exists only inside `mod tests` must never reach a guard"
    );
    assert!(
        !scoped.contains(TEST_MOD_MARKER),
        "the production slice must end at the test-module marker"
    );
    assert!(
        scoped.contains("fn ship"),
        "the production slice must keep the production item"
    );

    let in_production = concat!(
        "fn ship(ctx: &CallerContext) {\n    let _ = ctx.owner();\n}\n\n",
        "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
    );
    assert_eq!(
        production(in_production).matches(TOKEN).count(),
        1,
        "a token in the production portion must still be counted"
    );

    let out_of_line = "fn ship(ctx: &CallerContext) {\n    let _ = ctx.owner();\n}\n";
    assert_eq!(
        production(out_of_line),
        out_of_line,
        "a file whose tests live out of line keeps its whole source"
    );
}

#[test]
fn context_scope_is_bounded_to_known_sites() {
    for (name, source, expected) in bounded() {
        let applied = source.matches("apply_caller_tab(&mut params)").count();
        assert_eq!(
            applied, expected,
            "{name}: caller-tab injection count changed; no command may gain or lose context"
        );
    }

    assert_eq!(
        qa_rs().matches("ctx.owner()").count(),
        5,
        "qa_cmd owner injections are frozen at five"
    );
    assert_eq!(
        orchestrator_rs()
            .matches("CallerContext::from_env().owner()")
            .count(),
        1,
        "orchestrator carries exactly one owner injection"
    );
    assert_eq!(
        agent_rs()
            .matches("CallerContext::from_env().owner()")
            .count(),
        1,
        "agent carries exactly one owner injection"
    );

    assert!(
        !gate_rs().contains("ctx.owner()"),
        "decision gates never carried an owner object and must not gain one"
    );
    assert!(
        !read_rs().contains("owner()") && !local_io_rs().contains("owner()"),
        "plan state/lanes and workspace.close carry caller_tab_id only"
    );
    assert!(
        !read_rs().contains("apply_trusted_session")
            && !local_io_rs().contains("apply_trusted_session")
            && !gate_rs().contains("apply_trusted_session")
            && !qa_rs().contains("apply_trusted_session"),
        "no bounded site may gain trusted-session authority"
    );
}

/// B3: context is applied per call site. A method-name allow-list inside
/// call_plan_rpc would be the forbidden policy layer.
#[test]
fn no_method_allowlist_was_introduced() {
    let body = call_plan_rpc_body();

    assert!(
        body.contains("method == \"plan.task.adopt_checkpoint\""),
        "the pre-existing trusted-session predicate must stay intact"
    );
    assert!(
        body.contains("method.starts_with(\"plan.task.\")")
            && body.contains("\"task.take\" | \"task.drop\""),
        "the pre-existing method predicate must stay intact"
    );

    assert!(
        !body.contains("CallerContext"),
        "CallerContext must not be applied inside call_plan_rpc"
    );
    assert!(
        !body.contains("apply_caller_tab") && !body.contains("owner()"),
        "call_plan_rpc must not gain caller-tab or owner injection"
    );
    assert_eq!(
        body.matches("inject_trusted_session(&mut params)").count(),
        2,
        "the two pre-existing trusted-session calls are frozen"
    );
    for method in [
        "plan.qa.queue",
        "plan.qa.next",
        "plan.qa.pass",
        "plan.qa.fail",
        "plan.qa.override",
        "plan.decision.ask",
        "plan.decision.answer",
        "plan.decision.ack_request",
        "plan.state",
        "workspace.close",
    ] {
        assert!(
            !body.contains(method),
            "{method} must not appear in call_plan_rpc — that is a method allow-list"
        );
    }
}

/// D1-D5 are preserved by explicit decision, not normalized. Each assertion
/// pins the disagreement so a later "tidy-up" has to argue with a red test.
#[test]
fn trust_disagreements_are_preserved() {
    // D1: `task take` refuses over mesh; `task drop` ships trust ungated.
    assert!(
        task_rs().contains(
            "task take requires a Tab on the target machine and is unavailable over mesh"
        ),
        "D1: task take must keep refusing over mesh"
    );
    let drop_arm = task_rs()
        .split("TaskAction::Drop { task_id } => {")
        .nth(1)
        .expect("D1: task drop arm must exist");
    assert!(
        drop_arm[..drop_arm
            .find("call(client, \"task.drop\"")
            .expect("drop calls task.drop")]
            .contains("inject_trusted_session(&mut params)"),
        "D1: task drop must keep shipping trusted session without a mesh gate"
    );

    // D2: maybe_inject_caller_tab stays un-gated while its gated siblings remain gated.
    assert!(
        !fn_body(plan_rs(), "pub(crate) fn maybe_inject_caller_tab").contains("is_remote"),
        "D2: caller-tab injection must stay un-gated"
    );
    assert!(
        !caller_context_rs().contains("is_remote"),
        "D2: CallerContext must not introduce a mesh gate the helpers never had"
    );
    assert!(
        plan_rs().contains("(!client.is_remote()).then(caller_tab_id).flatten()"),
        "D2: the gated caller_tab_id site must stay gated"
    );
    assert!(
        fn_body(
            task_rs(),
            "fn caller_tab(client: &dyn RpcClient) -> Option<String>"
        )
        .contains("client.is_remote()"),
        "D2: task.rs caller_tab must stay is_remote-gated"
    );

    // D3: agent.report says caller_session_id where inject_trusted_session says session_id.
    assert!(
        agent_rs().contains("body[\"caller_session_id\"]"),
        "D3: agent report must keep the caller_session_id key name"
    );
    assert!(
        plan_rs().contains("fn inject_trusted_session"),
        "D3: the session_id-named helper must survive alongside it"
    );

    // D4: two owner shapes coexist.
    assert!(
        task_rs().contains("params[\"owner\"] = json!({ \"tab_id\": caller });"),
        "D4: task.rs must keep its tab_id-only owner shape"
    );
    assert_eq!(
        with_env(&full_env(), owner_from_env),
        frozen_owner(),
        "D4: plan.rs must keep its three-key owner shape"
    );

    // D5: agent.report and workspace.close bypass call_plan_rpc entirely.
    assert!(
        agent_rs().contains("client.call_detailed(\"agent.report\", Some(body.clone()))"),
        "D5: agent.report must keep bypassing call_plan_rpc"
    );
    assert!(
        local_io_rs().contains("client.call(\"workspace.close\", Some(params))"),
        "D5: workspace.close must keep bypassing call_plan_rpc"
    );
}

/// Negative controls: commands that deliberately carry no caller context.
#[test]
fn commands_without_context_stay_without_it() {
    let list = gate_rs()
        .split("PlanDecisionAction::List { plan_id, open } => {")
        .nth(1)
        .expect("decision list arm must exist");
    let list = &list[..list.find("call_plan_rpc").expect("list calls the rpc")];
    assert!(
        !list.contains("apply_caller_tab") && !list.contains("owner()"),
        "plan.decision.list carries no caller context and must not gain any"
    );

    let escalate = gate_rs()
        .split("PlanDecisionAction::Escalate {")
        .nth(1)
        .expect("decision escalate arm must exist");
    let escalate = &escalate[..escalate
        .find("call_plan_rpc")
        .expect("escalate calls the rpc")];
    assert!(
        !escalate.contains("apply_caller_tab") && !escalate.contains("owner()"),
        "plan.decision.escalate carries no caller context and must not gain any"
    );

    let mut arms = local_io_rs().split("WorkspaceAction::");
    arms.next();
    let mut seen = 0;
    for arm in arms {
        seen += 1;
        let carries = arm.contains("CallerContext") || arm.contains("apply_caller_tab");
        assert_eq!(
            carries,
            arm.starts_with("Close"),
            "only workspace.close carries caller context; arm `{}` disagrees",
            arm.split_whitespace().next().unwrap_or("?")
        );
    }
    assert_eq!(seen, 7, "workspace arm count is frozen");
}
