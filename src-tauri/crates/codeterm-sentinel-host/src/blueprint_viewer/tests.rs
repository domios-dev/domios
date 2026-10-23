use super::*;
use std::path::Path;

fn fixture(root: &Path, filename: &str, name: &str) {
    let directory = root.join(".domios/blueprints");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(filename), format!("schema_version: 2\nkind: pipeline\nname: {name}\nintent: inspect a repository source\ntrigger: {{type: schedule, every: {{lit: 1m}}}}\nsteps: [{{id: inspect, run: {{lit: 'printf ready'}}}}]\nresult: {{to: owner}}\n")).unwrap();
}
#[test]
fn blueprint_v2_project_list_and_view_are_read_only_and_reject_ambiguous_sources() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid.yaml", "valid");
    fixture(temp.path(), "mismatch.yaml", "wrong");
    let before = std::fs::read(temp.path().join(".domios/blueprints/valid.yaml")).unwrap();
    let entries = blueprint_project::list(temp.path()).unwrap();
    assert_eq!(entries.len(), 2);
    let valid = entries
        .iter()
        .find(|entry| entry.name.as_deref() == Some("valid"))
        .unwrap();
    assert_eq!(valid.path, ".domios/blueprints/valid.yaml");
    assert_eq!(valid.intent.as_deref(), Some("inspect a repository source"));
    assert!(entries
        .iter()
        .find(|entry| entry.path.ends_with("mismatch.yaml"))
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("stem"));
    let view = blueprint_project::show(temp.path(), &valid.path).unwrap();
    assert_eq!(view.document.name, "valid");
    assert_eq!(
        before,
        std::fs::read(temp.path().join(&valid.path)).unwrap()
    );
    for path in [
        "../valid.yaml",
        ".domios/blueprints/../valid.yaml",
        "/tmp/valid.yaml",
        "valid.yaml",
    ] {
        assert!(
            blueprint_project::show(temp.path(), path).is_err(),
            "{path}"
        );
    }
    fixture(temp.path(), "valid.yml", "valid");
    assert!(blueprint_project::show(temp.path(), &valid.path)
        .unwrap_err()
        .contains("Ambiguous"));
    assert_eq!(
        blueprint_project::list(temp.path())
            .unwrap()
            .iter()
            .filter(|entry| entry
                .error
                .as_deref()
                .is_some_and(|error| error.contains("Ambiguous")))
            .count(),
        2
    );
}
#[cfg(unix)]
#[test]
fn blueprint_v2_project_refuses_file_and_directory_links() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid.yaml", "valid");
    fixture(outside.path(), "escape.yaml", "escape");
    std::os::unix::fs::symlink(
        outside.path().join(".domios/blueprints/escape.yaml"),
        temp.path().join(".domios/blueprints/escape.yaml"),
    )
    .unwrap();
    assert!(blueprint_project::show(temp.path(), ".domios/blueprints/escape.yaml").is_err());
    assert!(blueprint_project::list(temp.path())
        .unwrap()
        .iter()
        .find(|entry| entry.path.ends_with("escape.yaml"))
        .unwrap()
        .error
        .is_some());
    let linked = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(temp.path().join(".domios"), linked.path().join(".domios")).unwrap();
    assert!(blueprint_project::list(linked.path()).is_err());
}
#[test]
fn blueprint_v2_project_host_uses_workspace_root_and_never_creates_a_job() {
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid.yaml", "valid");
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    mux.get_workspace_mut(&workspace).unwrap().cwd =
        Some(temp.path().to_string_lossy().into_owned());
    let service = ServiceFixture::new(mux);
    let listing = service
        .service
        .blueprint_project_host(&workspace, None)
        .unwrap();
    assert_eq!(listing[0]["name"], "valid");
    assert_eq!(
        service
            .service
            .blueprint_project_host(&workspace, Some(".domios/blueprints/valid.yaml"))
            .unwrap()["document"]["schema_version"],
        2
    );
    assert!(service
        .service
        .blueprint_project_host("foreign", None)
        .is_err());
    assert!(service.service.host_extension::<Handle>().is_none());
}

#[test]
fn blueprint_v2_project_history_projects_real_semantic_changes_and_bounds_bodies() {
    use crate::store::{Owner, Store};
    use codeterm_service_core::cron::clock::MockClock;
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(
        temp.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let source = json!({"schema_version":2,"kind":"pipeline","name":"history","owner":"team","trigger":{"type":"schedule","every":{"lit":"1m"}},"steps":[{"id":"emit","run":{"lit":"printf ready"},"out":{"type":"string"}}],"result":{"to":"owner"}});
    let mut job = store
        .prepare_document(
            source.clone(),
            Owner {
                creator_tab_id: "tab".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
                cwd: temp.path().to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    let before = amend::DefinitionSnapshot::from_job(&job);
    let mut after = source;
    after["steps"][0]["run"] = json!({"lit":"printf changed"});
    after["steps"][0]["routes"] = json!({"done":"@stop"});
    after["steps"][0]["out"] = json!({"type":"integer"});
    after["steps"][0]["caps"] = json!(["fs-read"]);
    job.document = Some(after);
    let history = amend::History {
        sentinel_id: job.id.clone(),
        current_version: 2,
        history_truncated: false,
        versions: vec![
            amend::DefinitionVersion {
                version: 1,
                provenance: "authored".into(),
                definition: before,
                amendment: None,
            },
            amend::DefinitionVersion {
                version: 2,
                provenance: "authored".into(),
                definition: amend::DefinitionSnapshot::from_job(&job),
                amendment: Some(amend::AmendmentLineage {
                    replaced_version: 1,
                    author_tab_id: "tab".into(),
                    cause_transition_id: None,
                    authored_at: 1001,
                }),
            },
        ],
    };
    let view = history_view(&history).unwrap();
    let changed = view["versions"][1]["diff"]["root"]["changed"]["emit"]
        .as_array()
        .unwrap();
    for field in ["run", "routes", "out", "caps"] {
        assert!(changed.contains(&json!(field)));
    }
    assert_eq!(
        view["versions"][1]["before"]["root"]["steps"][0]["run"]["lit"],
        "printf ready"
    );
    assert_eq!(
        view["versions"][1]["after"]["root"]["steps"][0]["run"]["lit"],
        "printf changed"
    );
}
