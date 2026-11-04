use super::*;
use crate::plan::Plan;

#[test]
fn job_count_includes_each_team_root_once_and_excludes_child_stages() {
    let root = |workspace, id, team, scenario_root| JobRoot {
        workspace,
        id,
        team,
        scenario_root,
    };
    let jobs = [
        root("workspace", "pipeline", Some("team"), true),
        root("workspace", "pipeline", Some("team"), true),
        root("workspace", "sentinel", Some("team"), true),
        root("workspace", "child", Some("team"), false),
        root("workspace", "foreign", Some("other"), true),
        root("workspace", "unowned", None, true),
        root("other-workspace", "pipeline", Some("team"), true),
    ];
    assert_eq!(count_job_roots("team", jobs.into_iter()), 3);
}

#[test]
fn counts_live_agents_and_idle_shells_and_names_only_finished_idle_workers() {
    let plan = Plan::new("P".into(), "/repo".into(), Default::default());
    let mut counters = crate::service::plan::digest_rules::plan_digest_counters(&plan, 0, 0);
    let row = || TeamTab {
        id: "tab",
        title: "Tidy",
        owner: false,
        agent: true,
        live: true,
        idle: true,
        finished_worker: true,
        idle_utility: false,
    };
    add_tab(
        &mut counters,
        TeamTab {
            owner: true,
            ..row()
        },
    );
    add_tab(
        &mut counters,
        TeamTab {
            live: false,
            ..row()
        },
    );
    add_tab(
        &mut counters,
        TeamTab {
            idle: false,
            ..row()
        },
    );
    add_tab(
        &mut counters,
        TeamTab {
            finished_worker: false,
            ..row()
        },
    );
    add_tab(&mut counters, row());
    add_tab(
        &mut counters,
        TeamTab {
            agent: false,
            idle_utility: true,
            ..row()
        },
    );
    add_tab(
        &mut counters,
        TeamTab {
            agent: false,
            idle_utility: false,
            ..row()
        },
    );
    assert_eq!(counters.agent_tabs, 3);
    assert_eq!(counters.idle_utility_shells, 1);
    assert_eq!(counters.idle_workers, ["Tidy (tab)"]);
}

#[test]
fn captured_inventory_keeps_the_target_binding_and_title_after_mux_changes() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TaskBinding;
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.team_id = Some("team".into());
        tab.role = Some(AgentRole::Worker);
        tab.title = "captured".into();
        tab.task_binding = Some(TaskBinding {
            plan_id: Some("plan".into()),
            task_id: Some("task".into()),
            ..Default::default()
        });
    }
    let fx = ServiceFixture::new(mux);
    let snapshot = snapshot_notice_inventory(&fx.service, "plan", Some("team"), Some(&owner));
    {
        let mut mux = fx.service.mux.write();
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.team_id = Some("foreign".into());
        tab.title = "changed".into();
        tab.task_binding.as_mut().unwrap().plan_id = Some("foreign-plan".into());
    }
    assert_eq!(snapshot.targets().tab_ids, [worker.clone()]);
    assert_eq!(snapshot.targets().task_ids, ["task"]);
    assert_eq!(snapshot.tabs[0].title, "captured");
    let current = snapshot_notice_inventory(&fx.service, "plan", Some("team"), Some(&owner));
    assert!(current.targets().tab_ids.is_empty());
}
