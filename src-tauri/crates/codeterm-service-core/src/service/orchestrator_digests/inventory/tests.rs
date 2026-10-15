use super::*;

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
