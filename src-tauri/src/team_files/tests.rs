use super::*;

fn root() -> PathBuf {
    std::env::temp_dir().join("domios-team-open")
}

#[test]
fn only_team_archives_are_claimed() {
    let base = root();
    let archive = base.join("desk.team");
    let upper = base.join("Desk.TEAM");
    assert_eq!(
        team_file_argument(archive.to_str().unwrap(), None),
        Some(archive)
    );
    assert_eq!(
        team_file_argument(upper.to_str().unwrap(), None),
        Some(upper)
    );
    for refused in [
        base.join("desk.team.yaml"),
        base.join("desk.yaml"),
        base.join("desk.team.zip"),
        base.join(".team"),
        base.join("team"),
    ] {
        assert_eq!(team_file_argument(refused.to_str().unwrap(), None), None);
    }
    for refused in ["", "--team", "-x.team", "domios://focus", "workspace"] {
        assert_eq!(team_file_argument(refused, None), None);
    }
}

#[test]
fn file_urls_and_relative_paths_resolve_to_absolute_packages() {
    let base = root();
    let archive = base.join("shared desk.team");
    let url = url::Url::from_file_path(&archive).unwrap();
    assert_eq!(
        team_file_argument(url.as_str(), None),
        Some(archive.clone())
    );
    let upper = url.as_str().replacen("file://", "FILE://", 1);
    assert_eq!(team_file_argument(&upper, None), Some(archive));
    let yaml = url::Url::from_file_path(base.join("desk.team.yaml")).unwrap();
    assert_eq!(team_file_argument(yaml.as_str(), None), None);
    assert_eq!(
        team_file_argument("desk.team", Some(base.as_path())),
        Some(base.join("desk.team"))
    );
}

#[test]
fn the_newest_existing_package_wins_and_missing_files_are_ignored() {
    let base = root();
    let first = base.join("first.team");
    let second = base.join("second.team");
    let missing = base.join("missing.team");
    let arguments = [
        "domios-app",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        missing.to_str().unwrap(),
        "--flag",
    ];
    let exists = |path: &Path| path != missing.as_path();
    assert_eq!(
        opened_team_file(arguments, None, exists),
        Some(second.clone())
    );
    assert_eq!(opened_team_file(arguments, None, |_| false), None);
    assert_eq!(
        opened_team_file(["domios-app", "workspace", "--help"], None, |_| true),
        None
    );
}

#[test]
fn a_waiting_package_is_taken_once_and_replaced_by_a_newer_one() {
    let slot = OpenedTeamFile::default();
    slot.replace(root().join("old.team"));
    slot.replace(root().join("new.team"));
    assert_eq!(slot.take(), Some(root().join("new.team")));
    assert_eq!(slot.take(), None);
}
