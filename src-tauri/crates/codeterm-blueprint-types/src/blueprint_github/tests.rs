use super::*;

#[test]
fn github_probe_uses_explicit_repo_and_read_only_resources() {
    for (resource, expected) in [
        (GithubResource::Run, "status,conclusion,url"),
        (GithubResource::Jobs, "status,conclusion,jobs,url"),
        (GithubResource::PrChecks, "statusCheckRollup,url"),
    ] {
        let command = GithubProbe {
            repo: "owner/project".into(),
            resource,
            id: 42,
        }
        .command()
        .unwrap();
        assert!(command.contains("--repo owner/project"));
        assert!(command.ends_with(expected));
    }
}

#[test]
fn github_probe_rejects_shell_syntax_and_invalid_scope() {
    for repo in [
        "owner",
        "owner/repo/extra",
        "owner/repo;whoami",
        "-R repo",
        "owner/$(id)",
        "/repo",
    ] {
        assert!(GithubProbe {
            repo: repo.into(),
            resource: GithubResource::Run,
            id: 42
        }
        .command()
        .is_err());
    }
    assert!(GithubProbe {
        repo: "owner/repo".into(),
        resource: GithubResource::Run,
        id: 0
    }
    .command()
    .is_err());
}
