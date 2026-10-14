use super::*;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};

fn browser_decl() -> SurfaceDecl {
    serde_json::from_value(
        json!({ "id": "browser", "title": "Browser", "renderer": "stream", "runtime": "browser" }),
    )
    .unwrap()
}

fn attach_surface(
    mux: &mut MultiplexerState,
    workspace: &str,
    owner: &str,
    surface_id: &str,
) -> String {
    let tab = mux.create_tab(workspace).unwrap();
    let surface = mux.get_tab_mut(&tab).unwrap();
    surface.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "web".into(),
        surface_id: surface_id.into(),
        instance_id: tab.clone(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    surface.parent_tab_id = Some(owner.to_string());
    surface.title = format!("{owner}'s Browser");
    tab
}

fn agent(mux: &mut MultiplexerState, workspace: &str, title: &str) -> String {
    let tab = mux.create_tab(workspace).unwrap();
    mux.rename_tab(&tab, title).unwrap();
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("claude".into());
    tab
}

struct Team {
    mux: MultiplexerState,
    workspace: String,
    snake: String,
    snake_browser: String,
    qa: String,
}

/// Snake owns a browser; QA, in the same workspace, has none.
fn team() -> Team {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let snake = agent(&mut mux, &workspace, "Snake");
    let qa = agent(&mut mux, &workspace, "QA");
    let snake_browser = attach_surface(&mut mux, &workspace, &snake, "browser");
    Team {
        mux,
        workspace,
        snake,
        snake_browser,
        qa,
    }
}

fn plan(team: &Team, caller: &str, verb: &str, of: Option<&str>) -> Result<CallPlan, String> {
    plan_agent_call(&team.mux, "web", &browser_decl(), caller, verb, of)
}

#[test]
fn surface_agent_call_reply_is_result_or_error_json() {
    let ok: Value = serde_json::from_str(&reply(Ok("done".into()))).unwrap();
    assert_eq!(ok, json!({ "result": "done" }));
    let err: Value = serde_json::from_str(&reply(Err("no".into()))).unwrap();
    assert_eq!(err, json!({ "error": "no" }));
}

#[test]
fn surface_agent_call_args_are_a_word_array() {
    assert_eq!(
        parse_verb_call(r#"["button","Sign in"]"#).unwrap().args,
        vec!["button".to_string(), "Sign in".to_string()]
    );
    assert_eq!(
        parse_verb_call(r#"[800]"#).unwrap().args,
        vec!["800".to_string()]
    );
    assert_eq!(parse_verb_call("").unwrap(), VerbCall::default());
    assert_eq!(parse_verb_call("null").unwrap(), VerbCall::default());
    assert!(parse_verb_call(r#"{"a":1}"#).is_err());
    assert!(parse_verb_call("7").is_err());
}

#[test]
fn surface_agent_call_args_object_carries_the_of_target() {
    assert_eq!(
        parse_verb_call(r#"{"args":["@e3"],"of":" QA "}"#).unwrap(),
        VerbCall {
            args: vec!["@e3".into()],
            of: Some("QA".into()),
        }
    );
    assert_eq!(
        parse_verb_call(r#"{"args":["@e3"]}"#).unwrap(),
        VerbCall {
            args: vec!["@e3".into()],
            of: None,
        }
    );
    assert!(parse_verb_call(r#"{"args":[],"of":""}"#).is_err());
    assert!(parse_verb_call(r#"{"args":[],"of":7}"#).is_err());
    assert!(parse_verb_call(r#"{"args":"x"}"#).is_err());
    assert!(parse_verb_call(r#"{"args":[],"of":"QA","as":"Snake"}"#).is_err());
}

#[test]
fn surface_agent_call_resolves_the_caller_by_tab_or_worker_id_and_finds_its_own_surface() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    mux.get_tab_mut(&owner).unwrap().worker_id = Some("w-7".into());
    assert_eq!(resolve_caller_tab(&mux, &owner), Some(owner.clone()));
    assert_eq!(resolve_caller_tab(&mux, "w-7"), Some(owner.clone()));
    assert_eq!(resolve_caller_tab(&mux, "nobody"), None);
    assert_eq!(resolve_caller_tab(&mux, " "), None);

    let other = mux.create_tab(&workspace).unwrap();
    for (parent, surface_id) in [(&other, "browser"), (&owner, "board"), (&owner, "browser")] {
        attach_surface(&mut mux, &workspace, parent, surface_id);
    }
    let found = caller_surface_tab(&mux, "web", "browser", &owner).expect("own surface");
    let tab = mux.get_tab(&found).unwrap();
    assert_eq!(tab.parent_tab_id.as_deref(), Some(owner.as_str()));
    assert_eq!(tab.surface().unwrap().surface_id, "browser");
    assert!(caller_surface_tab(&mux, "other", "browser", &owner).is_none());
}

#[test]
fn surface_agent_call_targets_the_first_stream_declaration() {
    let view: SurfaceDecl =
        serde_json::from_value(json!({ "id": "board", "title": "Board", "renderer": "view" }))
            .unwrap();
    assert_eq!(
        plugin_stream_surface(&[view.clone(), browser_decl()]).map(|s| s.id.as_str()),
        Some("browser")
    );
    assert!(plugin_stream_surface(&[view]).is_none());
}

#[test]
fn surface_agent_call_without_of_keeps_the_callers_own_browser() {
    let team = team();
    assert_eq!(
        plan(&team, &team.snake, "click", None),
        Ok(CallPlan::Run {
            tab_id: team.snake_browser.clone(),
            actor: None,
        })
    );
    assert_eq!(
        plan(&team, &team.snake, "close", None),
        Ok(CallPlan::Close {
            tab_id: team.snake_browser.clone(),
        })
    );
    assert_eq!(plan(&team, &team.qa, "open", None), Ok(CallPlan::Open));
    assert!(matches!(
        plan(&team, &team.qa, "status", None),
        Ok(CallPlan::Answer(_))
    ));
    assert!(plan(&team, &team.qa, "click", None)
        .unwrap_err()
        .contains("no browser open"));
}

#[test]
fn surface_agent_call_of_resolves_by_tab_id_title_or_surface_id_and_names_the_actor() {
    let team = team();
    let expected = Ok(CallPlan::Run {
        tab_id: team.snake_browser.clone(),
        actor: Some("QA".into()),
    });
    for of in [
        team.snake.as_str(),
        "Snake",
        "snake",
        " SNAKE ",
        team.snake_browser.as_str(),
    ] {
        assert_eq!(
            plan(&team, &team.qa, "click", Some(of)),
            expected,
            "--of {of}"
        );
    }
    assert_eq!(
        plan(&team, &team.snake, "click", Some("Snake")),
        Ok(CallPlan::Run {
            tab_id: team.snake_browser.clone(),
            actor: None,
        })
    );
}

#[test]
fn surface_agent_call_of_lists_candidates_when_the_title_is_ambiguous() {
    let mut team = team();
    let twin = agent(&mut team.mux, &team.workspace.clone(), "Twin");
    let other = agent(&mut team.mux, &team.workspace.clone(), "Other");
    team.mux.get_tab_mut(&other).unwrap().title = "twin".into();
    let error = plan(&team, &team.qa, "click", Some("Twin")).unwrap_err();
    assert!(error.contains("names 2 agents"), "{error}");
    assert!(error.contains(&twin) && error.contains(&other), "{error}");
    assert!(plan(&team, &team.qa, "click", Some(&twin))
        .unwrap_err()
        .contains("has no browser"));
}

#[test]
fn surface_agent_call_of_refuses_unknown_agents_and_owners_without_a_browser() {
    let team = team();
    assert!(plan(&team, &team.qa, "click", Some("Nobody"))
        .unwrap_err()
        .contains("no agent named Nobody"));
    let error = plan(&team, &team.snake, "open", Some("QA")).unwrap_err();
    assert_eq!(error, "QA has no browser; it opens its own with `open`");
    assert!(plan(&team, &team.qa, "status", Some(" ")).is_err());
}

#[test]
fn surface_agent_call_of_stays_inside_the_callers_workspace() {
    let mut team = team();
    let elsewhere = team.mux.create_workspace("Elsewhere".into(), None).id;
    let stranger = agent(&mut team.mux, &elsewhere, "Stranger");
    attach_surface(&mut team.mux, &elsewhere, &stranger, "browser");
    for of in ["Stranger", stranger.as_str()] {
        let error = plan(&team, &team.qa, "click", Some(of)).unwrap_err();
        assert!(error.contains("another workspace"), "--of {of}: {error}");
    }
    let error = plan(&team, &stranger, "click", Some("Snake")).unwrap_err();
    assert!(error.contains("another workspace"), "{error}");
}

#[test]
fn surface_agent_call_of_is_refused_to_mesh_controlled_callers() {
    let mut team = team();
    team.mux.get_tab_mut(&team.qa).unwrap().ownership =
        Some(codeterm_types::pty::RemoteOwnership {
            controller_title: "Remote".into(),
            controller_tab_id: None,
            peer_id: "peer-1".into(),
            peer_name: "Mac".into(),
            team: None,
        });
    assert!(plan(&team, &team.qa, "click", Some("Snake"))
        .unwrap_err()
        .contains("mesh peer"));
    assert!(matches!(
        plan(&team, &team.qa, "open", None),
        Ok(CallPlan::Open)
    ));
}

#[test]
fn surface_agent_call_of_cannot_close_another_agents_browser() {
    let team = team();
    assert!(plan(&team, &team.qa, "close", Some("Snake"))
        .unwrap_err()
        .contains("close only closes your own browser"));
}

#[test]
fn surface_agent_call_of_runs_every_verb_on_the_owners_path_with_the_actor() {
    let team = team();
    for verb in [
        "open",
        "status",
        "goto",
        "snapshot",
        "click",
        "type",
        "screenshot",
        "annotate",
    ] {
        let foreign = plan(&team, &team.qa, verb, Some("Snake")).unwrap();
        assert_eq!(
            foreign,
            CallPlan::Run {
                tab_id: team.snake_browser.clone(),
                actor: Some("QA".into()),
            },
            "{verb}"
        );
        if verb != "annotate" {
            assert_eq!(
                plan(&team, &team.snake, verb, None).unwrap(),
                CallPlan::Run {
                    tab_id: team.snake_browser.clone(),
                    actor: None,
                },
                "owner parity for {verb}"
            );
        }
    }
}

#[test]
fn surface_agent_call_annotate_refuses_the_owners_own_browser() {
    let team = team();
    for of in [None, Some("Snake")] {
        let error = plan(&team, &team.snake, "annotate", of).unwrap_err();
        assert_eq!(
            error,
            "annotate targets another agent's browser; use --of <agent>"
        );
    }
}

#[test]
fn surface_agent_call_annotation_goes_to_the_surface_owner() {
    let team = team();
    let binding = super::super::stream_ops::stream_binding(&team.mux, &team.snake_browser, |_| {
        Some(vec![browser_decl()])
    })
    .unwrap();
    assert_eq!(annotation_recipient(&binding), Ok(team.snake.as_str()));
    assert_eq!(binding.owner_name, "Snake");
    let orphan = StreamBinding {
        owner_tab_id: None,
        ..binding
    };
    assert!(annotation_recipient(&orphan).is_err());
}
