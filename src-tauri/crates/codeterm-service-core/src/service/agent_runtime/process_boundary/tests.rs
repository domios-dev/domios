use super::*;

#[test]
fn retirement_requires_boundary_termination_and_confirmed_exit() {
    for (terminated, exited, confirmed) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        assert_eq!(retirement_confirmed(terminated, exited), confirmed);
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
#[test]
fn attribution_requires_exact_command_arguments_and_cwd() {
    let cwd = std::path::Path::new("/work");
    let other = std::path::Path::new("/foreign");
    let expected = args(&["node", "-e", "opaque"]);
    let observed = args(&["/bin/node", "-e", "opaque"]);
    assert!(command_matches(
        &observed,
        cwd,
        &expected,
        cwd,
        "node -e opaque"
    ));
    assert!(!command_matches(
        &observed,
        other,
        &expected,
        cwd,
        "node -e opaque"
    ));
    assert!(!command_matches(
        &args(&["/bin/node", "-e", "other"]),
        cwd,
        &expected,
        cwd,
        "node -e opaque"
    ));
    assert!(!command_matches(
        &observed,
        cwd,
        &args(&["/different/node", "-e", "opaque"]),
        cwd,
        "node -e opaque"
    ));
    assert!(command_matches(
        &args(&["/bin/sh", "-c", "node -e opaque"]),
        cwd,
        &expected,
        cwd,
        "node -e opaque"
    ));
    assert!(!command_matches(
        &args(&["/bin/sh", "-c", "prefix node -e opaque"]),
        cwd,
        &expected,
        cwd,
        "node -e opaque"
    ));
    assert!(!command_matches(
        &args(&["pwsh", "-File", "node -e opaque"]),
        cwd,
        &expected,
        cwd,
        "node -e opaque"
    ));
}

#[test]
fn root_cwd_remains_attributable() {
    assert!(super::command_matches(
        &args(&["node", "worker"]),
        std::path::Path::new("/"),
        &args(&["node", "worker"]),
        std::path::Path::new("/"),
        "node worker"
    ));
}

#[cfg(unix)]
#[test]
fn non_utf8_cwd_never_aliases_a_roster_path() {
    use std::os::unix::ffi::OsStrExt;
    let cwd = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/work/\xff"));
    assert!(!super::command_matches(
        &args(&["node"]),
        cwd,
        &args(&["node"]),
        std::path::Path::new("/work/�"),
        "node"
    ));
}
