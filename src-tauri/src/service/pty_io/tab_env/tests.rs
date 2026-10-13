use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, Plan, PlannerRef};
use crate::service::plan::PlanStore;
use crate::service::pty_io::tests::store_with_bound_task;
use std::collections::BTreeSet;

const PLANNER_TAB_ID: &str = "tab-planner";

fn store_with_planner_bound_plan() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Planner-bound".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: PLANNER_TAB_ID.into(),
        session_id: None,
        title: Some("Planner".into()),
    });
    store.save(&mut plan, 1).unwrap();
    (dir, store, plan.id)
}

const AGENT_TAB_ID: &str = "tab-a";

#[test]
fn codex_rpc_sidecar_env_never_rewrites_agent_toolchain_selection() {
    let env = compose_launch_env(&instance_env(), None, LaunchSurface::Service);
    let mut command = std::process::Command::new("agent");
    let inherited = [
        ("CODEX_HOME", "/custom/account"),
        ("HOME", "/custom/home"),
        ("CODEX_MANAGED_PACKAGE_ROOT", "/custom/toolchain"),
        ("CODEX_MANAGED_BY_NPM", "1"),
    ];
    command.envs(inherited).envs(&env);
    let effective: HashMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(|value| value.to_owned())))
        .collect();
    for (key, value) in inherited {
        assert!(!env.contains_key(key));
        assert_eq!(
            effective.get(std::ffi::OsStr::new(key)).unwrap().as_deref(),
            Some(std::ffi::OsStr::new(value))
        );
    }
}
const LANE_BASE: &str = "release/1.10.0";

fn instance_env() -> InstanceEnv {
    InstanceEnv {
        socket_path: "/tmp/codeterm-test.sock".into(),
        api_url: "http://127.0.0.1:7686".into(),
        api_port: "7686".into(),
        api_token: Some("test-token".into()),
        cli_path: None,
    }
}

/// Identity as a live tab record carries it: team, role, parent, worker.
fn agent_identity() -> HashMap<String, String> {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&tab_id).unwrap();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.team_id = Some("team_c4bc03af".into());
        tab.parent_tab_id = Some("tab-parent".into());
        tab.worker_id = Some("worker-7".into());
    }
    tab_identity_env(mux.get_tab(&tab_id).unwrap())
}

fn plain_shell_identity() -> HashMap<String, String> {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    tab_identity_env(mux.get_tab(&tab_id).unwrap())
}

fn keys(env: &HashMap<String, String>) -> BTreeSet<String> {
    env.keys().cloned().collect()
}

fn without_frontend_only(env: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = env.clone();
    for (key, _) in FRONTEND_ONLY_ENV {
        env.remove(*key);
    }
    env
}

/// Set equality over the whole composed map, so a variable added to one path
/// and not the other fails here instead of in a restored tab.
#[test]
fn env_compose_both_launch_paths_agree_for_one_tab_record() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task(LANE_BASE);
    let tab = TabLaunchEnv {
        tab_id: AGENT_TAB_ID.into(),
        pad_dir: Some("/pads/tab-a".into()),
        session_binding_nonce: "nonce-binding".into(),
        shell_nonce: "nonce-shell".into(),
        identity: agent_identity(),
        task: tab_task_env(&store, AGENT_TAB_ID),
    };
    let instance = instance_env();

    let service = compose_launch_env(&instance, Some(&tab), LaunchSurface::Service);
    let frontend = compose_launch_env(&instance, Some(&tab), LaunchSurface::Frontend);

    assert_eq!(
        without_frontend_only(&frontend),
        service,
        "both launch paths must compose the same env for the same tab record"
    );
    let expected_frontend_keys: BTreeSet<String> = keys(&service)
        .into_iter()
        .chain(FRONTEND_ONLY_ENV.iter().map(|(key, _)| key.to_string()))
        .collect();
    assert_eq!(
        keys(&frontend),
        expected_frontend_keys,
        "the frontend surface may differ only by FRONTEND_ONLY_ENV"
    );
}

/// Census case 4: a tab materialised by the post-update resume path is the
/// frontend surface, and must come back with the identity its record holds.
#[test]
fn env_compose_restore_path_carries_plan_task_team_lane_and_pad() {
    let (_dir, store, plan_id, task_id) = store_with_bound_task(LANE_BASE);
    let tab = TabLaunchEnv {
        tab_id: AGENT_TAB_ID.into(),
        pad_dir: Some("/pads/tab-a".into()),
        session_binding_nonce: "nonce-binding".into(),
        shell_nonce: "nonce-shell".into(),
        identity: agent_identity(),
        task: tab_task_env(&store, AGENT_TAB_ID),
    };

    let resumed = compose_launch_env(&instance_env(), Some(&tab), LaunchSurface::Frontend);

    assert_eq!(
        resumed.get("CODETERM_PLAN_ID").map(String::as_str),
        Some(plan_id.as_str())
    );
    assert_eq!(
        resumed.get("CODETERM_TASK_ID").map(String::as_str),
        Some(task_id.as_str())
    );
    assert_eq!(
        resumed.get("CODETERM_TEAM_ID").map(String::as_str),
        Some("team_c4bc03af")
    );
    assert_eq!(
        resumed.get("CODETERM_LANE_BASE").map(String::as_str),
        Some(LANE_BASE)
    );
    assert_eq!(
        resumed.get("CODETERM_PAD_DIR").map(String::as_str),
        Some("/pads/tab-a")
    );
}

/// A record without team/plan/task composes without those variables, and a
/// blank input is dropped rather than exported — the CLI tests for non-empty.
#[test]
fn env_compose_absent_identity_omits_variables_instead_of_blanking_them() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task(LANE_BASE);
    let mut identity = plain_shell_identity();
    identity.insert("CODETERM_TEAM_ID".into(), String::new());
    let tab = TabLaunchEnv {
        tab_id: "plain-tab".into(),
        pad_dir: None,
        session_binding_nonce: "nonce-binding".into(),
        shell_nonce: "nonce-shell".into(),
        identity,
        task: tab_task_env(&store, "plain-tab"),
    };
    let instance = instance_env();

    let service = compose_launch_env(&instance, Some(&tab), LaunchSurface::Service);
    let frontend = compose_launch_env(&instance, Some(&tab), LaunchSurface::Frontend);

    for absent in [
        "CODETERM_TEAM_ID",
        "CODETERM_PLAN_ID",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
        "CODETERM_LANE_BASE",
        "CODETERM_PAD_DIR",
        "CODETERM_ROLE",
        "CODETERM_PARENT_TAB_ID",
        "CODETERM_WORKER_ID",
    ] {
        assert!(
            !service.contains_key(absent),
            "{absent} must be absent, not blank, for a record that has none"
        );
    }
    assert!(
        service
            .values()
            .chain(frontend.values())
            .all(|v| !v.is_empty()),
        "no launch variable may be exported with an empty value"
    );
    assert_eq!(without_frontend_only(&frontend), service);
}

/// Byte-identity control for the fuller path: an agent tab launched by the
/// daemon stamps exactly what it stamps today, and never CODETERM_VERSION.
#[test]
fn env_compose_service_surface_matches_todays_agent_launch_map() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task(LANE_BASE);
    let tab = TabLaunchEnv {
        tab_id: AGENT_TAB_ID.into(),
        pad_dir: Some("/pads/tab-a".into()),
        session_binding_nonce: "nonce-binding".into(),
        shell_nonce: "nonce-shell".into(),
        identity: agent_identity(),
        task: tab_task_env(&store, AGENT_TAB_ID),
    };

    let service = compose_launch_env(&instance_env(), Some(&tab), LaunchSurface::Service);

    let expected: BTreeSet<String> = [
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_LANE_BASE",
        "CODETERM_PAD_DIR",
        "CODETERM_PARENT_TAB_ID",
        "CODETERM_PLAN_ID",
        "CODETERM_ROLE",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_SHELL_NONCE",
        "CODETERM_SOCKET_PATH",
        "CODETERM_TAB_ID",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
        "CODETERM_TEAM_ID",
        "CODETERM_WORKER_ID",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(keys(&service), expected);
    assert_eq!(
        service.get("CODETERM_TAB_ID").map(String::as_str),
        Some(AGENT_TAB_ID)
    );
    assert_eq!(
        service.get("CODETERM_ROLE").map(String::as_str),
        Some("Manager")
    );
}

/// The frontend surface also spawns PTYs that have no tab record yet; those
/// carry instance vars only, never a half-filled identity.
#[test]
fn env_compose_frontend_spawn_without_tab_record_stamps_no_identity() {
    let frontend = compose_launch_env(&instance_env(), None, LaunchSurface::Frontend);

    let expected: BTreeSet<String> = [
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_SOCKET_PATH",
    ]
    .into_iter()
    .map(str::to_string)
    .chain(FRONTEND_ONLY_ENV.iter().map(|(key, _)| key.to_string()))
    .collect();
    assert_eq!(keys(&frontend), expected);
}

/// codeterm#371: a Planner binds to its plan directly, never a task.
#[test]
fn planner_bound_tab_env_carries_plan_id_without_a_task_id() {
    let (_dir, store, plan_id) = store_with_planner_bound_plan();

    let task_env = tab_task_env(&store, PLANNER_TAB_ID);

    assert_eq!(
        task_env.get("CODETERM_PLAN_ID").map(String::as_str),
        Some(plan_id.as_str())
    );
    assert!(
        !task_env.contains_key("CODETERM_TASK_ID"),
        "a Planner holds no task, so no CODETERM_TASK_ID must be stamped"
    );
}

/// Neither binding shape guesses at the other's absence.
#[test]
fn unbound_tab_env_carries_no_plan_identity() {
    let (_dir, store, _plan_id) = store_with_planner_bound_plan();

    let task_env = tab_task_env(&store, "tab-unrelated");

    assert!(task_env.get("CODETERM_PLAN_ID").is_none());
    assert!(task_env.get("CODETERM_TASK_ID").is_none());
}

#[test]
fn task_env_selects_own_open_attempt_and_lane_base() {
    use crate::plan::TaskExecutionAttempt;
    let own = TaskExecutionAttempt {
        id: "current-attempt".into(),
        executor_tab_id: AGENT_TAB_ID.into(),
        base: Some("attempt-base".into()),
        ..Default::default()
    };
    let attempts = [
        own.clone(),
        TaskExecutionAttempt {
            id: "closed-attempt".into(),
            closed_at: Some(4),
            ..own.clone()
        },
        TaskExecutionAttempt {
            id: "retired-attempt".into(),
            retired_at: Some(5),
            ..own.clone()
        },
        TaskExecutionAttempt {
            id: "foreign-attempt".into(),
            executor_tab_id: "other-tab".into(),
            ..own
        },
    ];
    let env = compose_task_env("plan", "task", AGENT_TAB_ID, &attempts, Some("lane-base"));
    assert_eq!(env["CODETERM_ATTEMPT_ID"], "current-attempt");
    assert_eq!(env["CODETERM_LANE_BASE"], "attempt-base");
    let env = compose_task_env("plan", "task", "missing-tab", &attempts, Some("lane-base"));
    assert!(!env.contains_key("CODETERM_ATTEMPT_ID"));
    assert_eq!(env["CODETERM_LANE_BASE"], "lane-base");
}

#[test]
fn dispatched_task_env_reconstructs_the_durable_attempt_id() {
    let (_dir, store, plan_id, task_id) = store_with_bound_task(LANE_BASE);
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    let attempt = task.execution_attempts.last().unwrap();
    assert_eq!(
        tab_task_env(&store, AGENT_TAB_ID)["CODETERM_ATTEMPT_ID"],
        attempt.id
    );
}

#[test]
fn launch_env_composes_task_identity_in_gui_and_normal_path_modes() {
    let attempt = crate::plan::TaskExecutionAttempt {
        id: "attempt".into(),
        executor_tab_id: AGENT_TAB_ID.into(),
        ..Default::default()
    };
    for path in [
        "/usr/bin:/bin",
        "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
    ] {
        for task_bound in [false, true] {
            let tab = TabLaunchEnv {
                tab_id: AGENT_TAB_ID.into(),
                pad_dir: None,
                session_binding_nonce: "binding".into(),
                shell_nonce: "shell".into(),
                identity: HashMap::from([("PATH".into(), path.into())]),
                task: if task_bound {
                    compose_task_env(
                        "plan",
                        "task",
                        AGENT_TAB_ID,
                        std::slice::from_ref(&attempt),
                        None,
                    )
                } else {
                    HashMap::new()
                },
            };
            for surface in [LaunchSurface::Service, LaunchSurface::Frontend] {
                let env = compose_launch_env(&instance_env(), Some(&tab), surface);
                for (key, value) in [
                    ("CODETERM_PLAN_ID", "plan"),
                    ("CODETERM_TASK_ID", "task"),
                    ("CODETERM_ATTEMPT_ID", "attempt"),
                ] {
                    assert_eq!(
                        env.get(key).map(String::as_str),
                        task_bound.then_some(value)
                    );
                }
                assert_eq!(env["PATH"], path);
            }
        }
    }
}

#[test]
fn agents_and_shells_receive_the_owning_instance_cli_with_restricted_path() {
    let mut instance = instance_env();
    let cli = if cfg!(windows) {
        r"C:\dev\bin\codeterm.exe"
    } else {
        "/dev-instance/bin/codeterm"
    };
    instance.cli_path = Some(cli.into());
    let cli_dir = std::path::Path::new(cli).parent().unwrap();
    let inherited = std::env::join_paths([std::path::PathBuf::from(if cfg!(windows) {
        r"C:\prod\bin"
    } else {
        "/prod-instance/bin"
    })])
    .unwrap();
    for surface in [LaunchSurface::Service, LaunchSurface::Frontend] {
        for identity in [agent_identity(), plain_shell_identity()] {
            let tab = TabLaunchEnv {
                tab_id: AGENT_TAB_ID.into(),
                pad_dir: None,
                session_binding_nonce: "binding".into(),
                shell_nonce: "shell".into(),
                identity,
                task: HashMap::new(),
            };
            let env = compose_launch_env(&instance, Some(&tab), surface);
            assert_eq!(env["CODETERM_CLI_PATH"], cli);
            let path = crate::pty::manager::path_with_cli_dir_first(
                &inherited,
                std::path::Path::new(&env["CODETERM_CLI_PATH"])
                    .parent()
                    .unwrap(),
            )
            .unwrap();
            let entries: Vec<_> = std::env::split_paths(&path).collect();
            assert_eq!(entries[0], cli_dir);
            assert_eq!(
                entries[1..],
                std::env::split_paths(&inherited).collect::<Vec<_>>()
            );
        }
    }
}
