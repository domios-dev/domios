use super::*;

#[test]
fn background_stop_requires_exact_shell_command_and_cwd() {
    #[cfg(windows)]
    let (cwd, other, program, switch, script) = (
        "C:/work",
        "C:/other",
        "C:/WindowsApps/pwsh.exe",
        "-Command",
        "try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\nnode target.cjs",
    );
    #[cfg(unix)]
    let (cwd, other, program, switch, script) = (
        "/work/project",
        "/work\\project",
        "/bin/bash",
        "-lc",
        "node target.cjs",
    );
    let cwd = Path::new(cwd);
    let argv = [program.into(), switch.into(), script.into()];
    assert!(shell_matches(&argv, cwd, "node target.cjs", cwd));
    assert!(!shell_matches(&argv, cwd, "node keeper.cjs", cwd));
    assert!(!shell_matches(&argv, cwd, "target.cjs", cwd));
    assert!(!shell_matches(
        &argv,
        cwd,
        "node target.cjs",
        Path::new(other)
    ));
    let arbitrary_wrapper = [
        program.into(),
        switch.into(),
        "echo setup\nnode target.cjs".into(),
    ];
    assert!(!shell_matches(
        &arbitrary_wrapper,
        cwd,
        "node target.cjs",
        cwd
    ));
}

fn node(pid: u32, parent: u32) -> Node {
    Node {
        pid,
        parent,
        infrastructure: false,
        exited: false,
    }
}

#[test]
fn background_stop_selects_only_the_target_subtree() {
    let mut nodes = [
        node(20, 10),
        node(21, 20),
        node(22, 21),
        node(30, 10),
        node(31, 30),
        node(40, 10),
    ];
    nodes[5].infrastructure = true;
    let protected = HashSet::from([3]);
    assert_eq!(subtree(&nodes, &[0], &protected), Some(vec![0, 1, 2]));
    assert_eq!(subtree(&nodes, &[], &protected), None);
    assert_eq!(subtree(&nodes, &[0, 3], &protected), None);
    assert_eq!(subtree(&nodes, &[0], &HashSet::from([2])), None);
    assert_eq!(subtree(&nodes, &[5], &protected), None);
    nodes[0].exited = true;
    assert_eq!(subtree(&nodes, &[0], &protected), None);
}
