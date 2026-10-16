use super::*;
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
