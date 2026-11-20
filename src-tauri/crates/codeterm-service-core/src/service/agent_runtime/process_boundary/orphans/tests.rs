use super::*;

const ROOT: u32 = 10;

fn node(pid: u32, parent: u32, exited: bool) -> Node {
    Node {
        pid,
        parent,
        infrastructure: false,
        exited,
    }
}

fn reaped(nodes: &[Node]) -> Vec<u32> {
    orphaned(nodes).into_iter().map(|i| nodes[i].pid).collect()
}

#[test]
fn stopped_shell_orphans_its_escaped_command_tree() {
    let nodes = [
        node(20, ROOT, true),
        node(30, 20, false),
        node(40, 30, false),
    ];
    assert_eq!(reaped(&nodes), [30, 40]);
}

#[test]
fn tools_whose_owned_ancestry_survived_are_retained() {
    let nodes = [node(20, ROOT, false), node(30, 20, false)];
    assert!(reaped(&nodes).is_empty());
}

#[test]
fn infrastructure_is_never_reaped_and_never_orphans() {
    let server = Node {
        infrastructure: true,
        ..node(20, ROOT, false)
    };
    let exited_helper = Node {
        infrastructure: true,
        ..node(50, ROOT, true)
    };
    let nodes = [
        server,
        node(30, 20, false),
        exited_helper,
        node(60, 50, false),
    ];
    assert!(reaped(&nodes).is_empty());
    let stopped_tool = node(70, 20, true);
    let nodes = [server, stopped_tool, node(80, 70, false)];
    assert_eq!(reaped(&nodes), [80]);
}

#[test]
fn processes_already_gone_are_not_reaped() {
    let nodes = [node(20, ROOT, true), node(30, 20, true)];
    assert!(reaped(&nodes).is_empty());
}

#[test]
fn parent_cycles_terminate_without_reaping() {
    let nodes = [node(20, 30, false), node(30, 20, false)];
    assert!(reaped(&nodes).is_empty());
}

#[test]
fn stop_is_not_complete_while_a_reaped_tool_survives() {
    assert!(confirm_reaped(&[]).is_ok());
    let error = confirm_reaped(&[30]).unwrap_err();
    assert!(error.contains("30"));
}
