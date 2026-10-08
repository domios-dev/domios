use super::*;

const TOKEN: &str = "guide-token-q7";

fn bundle() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/AGENT.md"), format!("# {TOKEN}\n\n")).unwrap();
    dir
}

#[test]
fn a_guide_inside_the_bundle_loads_trimmed() {
    let dir = bundle();
    let guide = load_agent_guide(dir.path(), "docs/AGENT.md").unwrap();
    assert_eq!(guide.text, format!("# {TOKEN}"));
    assert!(!guide.truncated);
    assert_eq!(
        load_agent_guide(dir.path(), "./docs/AGENT.md").unwrap(),
        guide
    );
}

#[test]
fn paths_that_leave_the_bundle_are_refused_before_reading() {
    let outer = tempfile::tempdir().unwrap();
    let bundle_dir = outer.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    std::fs::write(outer.path().join("secret.md"), TOKEN).unwrap();
    let absolute = outer.path().join("secret.md").to_string_lossy().to_string();
    for path in [
        "../secret.md",
        "docs/../../secret.md",
        absolute.as_str(),
        "/etc/passwd",
        "C:/secret.md",
        "..\\secret.md",
        "",
        " docs/AGENT.md",
    ] {
        assert_eq!(
            load_agent_guide(&bundle_dir, path),
            Err(AgentGuideError::InvalidPath),
            "{path:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_that_escapes_the_bundle_is_refused() {
    let outer = tempfile::tempdir().unwrap();
    let bundle_dir = outer.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    std::fs::write(outer.path().join("secret.md"), TOKEN).unwrap();
    std::os::unix::fs::symlink(outer.path().join("secret.md"), bundle_dir.join("AGENT.md"))
        .unwrap();
    assert_eq!(
        load_agent_guide(&bundle_dir, "AGENT.md"),
        Err(AgentGuideError::OutsideBundle)
    );
}

#[test]
fn a_missing_or_directory_guide_is_missing() {
    let dir = bundle();
    assert_eq!(
        load_agent_guide(dir.path(), "docs/NOPE.md"),
        Err(AgentGuideError::Missing)
    );
    assert_eq!(
        load_agent_guide(dir.path(), "docs"),
        Err(AgentGuideError::Missing)
    );
}

#[test]
fn an_oversized_guide_is_cut_to_the_bound_with_a_marker() {
    let dir = bundle();
    let body = format!("{TOKEN}x{}", "é".repeat(MAX_AGENT_GUIDE_BYTES));
    std::fs::write(dir.path().join("big.md"), &body).unwrap();
    let guide = load_agent_guide(dir.path(), "big.md").unwrap();
    assert!(guide.truncated);
    assert!(guide.text.starts_with(TOKEN));
    assert!(guide.text.ends_with(AGENT_GUIDE_TRUNCATED_MARKER));
    assert!(guide.text.len() <= MAX_AGENT_GUIDE_BYTES + AGENT_GUIDE_TRUNCATED_MARKER.len());
}

#[test]
fn a_guide_exactly_at_the_bound_is_whole() {
    let dir = bundle();
    std::fs::write(
        dir.path().join("full.md"),
        "x".repeat(MAX_AGENT_GUIDE_BYTES),
    )
    .unwrap();
    let guide = load_agent_guide(dir.path(), "full.md").unwrap();
    assert!(!guide.truncated);
    assert_eq!(guide.text.len(), MAX_AGENT_GUIDE_BYTES);
}

#[test]
fn non_utf8_and_blank_guides_are_refused() {
    let dir = bundle();
    std::fs::write(dir.path().join("bin.md"), [0xff, 0xfe, b'a']).unwrap();
    std::fs::write(dir.path().join("blank.md"), "  \n\n").unwrap();
    assert_eq!(
        load_agent_guide(dir.path(), "bin.md"),
        Err(AgentGuideError::NotUtf8)
    );
    assert_eq!(
        load_agent_guide(dir.path(), "blank.md"),
        Err(AgentGuideError::Empty)
    );
}
