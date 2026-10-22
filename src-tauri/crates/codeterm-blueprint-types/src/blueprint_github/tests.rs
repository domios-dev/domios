use super::*;

#[test]
fn github_probe_uses_explicit_repo_and_read_only_resources() {
    for (resource, expected) in [
        (GithubResource::Run, "status,conclusion,jobs,url"),
        (GithubResource::Jobs, "status,conclusion,jobs,url"),
        (GithubResource::PrChecks, "statusCheckRollup,url"),
    ] {
        let command = GithubProbe {
            repo: "owner/project".into(),
            resource,
            id: 42,
            org: None,
            label: None,
            timeout_ms: 60000,
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
            id: 42,
            org: None,
            label: None,
            timeout_ms: 60000,
        }
        .command()
        .is_err());
    }
    assert!(GithubProbe {
        repo: "owner/repo".into(),
        resource: GithubResource::Run,
        id: 0,
        org: None,
        label: None,
        timeout_ms: 60000,
    }
    .command()
    .is_err());
}

#[test]
fn github_runner_scope_is_explicit_and_cannot_inject_commands() {
    for (scope, endpoint) in [
        (
            serde_json::json!({"repo":"owner/project"}),
            "repos/owner/project",
        ),
        (serde_json::json!({"org":"owner"}), "orgs/owner"),
    ] {
        let mut value = scope;
        value["resource"] = "runners".into();
        value["label"] = "linux-build".into();
        let probe: GithubProbe = serde_json::from_value(value).unwrap();
        assert!(probe.command().unwrap().contains(endpoint));
    }
    for value in [
        serde_json::json!({"repo":"owner/repo","org":"owner","resource":"runners"}),
        serde_json::json!({"org":"owner;whoami","resource":"runners"}),
        serde_json::json!({"repo":"owner/repo","resource":"runners","id":42}),
    ] {
        assert!(serde_json::from_value::<GithubProbe>(value)
            .unwrap()
            .command()
            .is_err());
    }
}
