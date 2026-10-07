use super::*;

#[test]
fn context_attach_support_is_capability_driven() {
    let files = TaskFiles {
        edit: vec!["new.rs".into()],
        read: vec![],
    };
    assert!(validate_support(&files, false, None).is_err());
    let edit_only = Some(ContextAttach {
        editable: true,
        read_only: false,
    });
    assert!(validate_support(&files, false, edit_only).is_ok());
    assert!(validate_support(&files, true, edit_only).is_err());
    assert!(validate_support(
        &files,
        true,
        Some(ContextAttach {
            editable: true,
            read_only: true
        })
    )
    .is_ok());
}

#[test]
fn context_files_resolve_deduplicate_and_refuse_alias_overlap() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("context.md"), "opaque").unwrap();
    let files = resolve_files(
        dir.path(),
        TaskFiles {
            edit: vec!["new.rs".into(), "./new.rs".into()],
            read: vec!["context.md".into()],
        },
    )
    .unwrap();
    assert_eq!(files.edit.len(), 1);
    assert!(Path::new(&files.edit[0]).is_absolute());
    assert!(resolve_files(
        dir.path(),
        TaskFiles {
            edit: vec!["./context.md".into()],
            read: vec!["context.md".into()]
        }
    )
    .is_err());
    assert!(resolve_files(
        dir.path(),
        TaskFiles {
            read: vec!["missing.md".into()],
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn plan_skill_bundle_requires_exact_attempt_digest_and_keeps_full_body() {
    let body = "---\npurpose: opaque\n---\nFull opaque body\n";
    let digest = format!("sha256:{:x}", Sha256::digest(body.as_bytes()));
    let mut bundle = String::new();
    append_skill(&mut bundle, "opaque", &digest, body).unwrap();
    assert!(bundle.ends_with(body));
    assert!(append_skill(&mut bundle, "opaque", &digest, "changed").is_err());
}

#[test]
fn context_bundle_forwards_files_and_materialized_skills_without_changing_brief() {
    let params = AgentLaunchParams {
        task: Some("opaque\nbrief".into()),
        context_files: Some(TaskFiles {
            edit: vec!["/opaque/new.rs".into()],
            read: vec!["/opaque/context.md".into()],
        }),
        ..Default::default()
    };
    let prepared = attach_skills(&params, Some("whole skill body"), |body| {
        assert_eq!(body, "whole skill body");
        Some("/opaque/skills.md".into())
    })
    .unwrap();
    assert_eq!(prepared.task, params.task);
    let files = prepared.context_files.unwrap();
    assert_eq!(files.edit, ["/opaque/new.rs"]);
    assert_eq!(files.read, ["/opaque/context.md", "/opaque/skills.md"]);
    assert!(attach_skills(&params, Some("opaque"), |_| None).is_none());
}

#[test]
fn resume_context_revalidates_files_and_rematerializes_shared_instructions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("read.md"), "opaque context").unwrap();
    let params = crate::agent::provider::AgentResumeParams {
        session_id: "existing-own-session".into(),
        cwd: Some(dir.path().to_string_lossy().into_owned()),
        context_files: Some(TaskFiles {
            edit: vec!["new.rs".into()],
            read: vec!["read.md".into()],
        }),
        ..Default::default()
    };
    let capability = Some(ContextAttach {
        editable: true,
        read_only: true,
    });
    let mut bodies = Vec::new();
    let prepared = prepare_resume_context(
        &params,
        Some("opaque skill"),
        true,
        crate::agent::provider::SystemPromptDelivery::External,
        capability,
        |body, suffix| {
            bodies.push(body.to_string());
            Some(format!("/opaque/{}", suffix.unwrap()))
        },
    )
    .unwrap();
    assert_eq!(prepared.session_id, params.session_id);
    assert_eq!(prepared.cwd, params.cwd);
    assert_eq!(bodies, ["opaque skill", crate::prompts::TOOL_LESS]);
    assert!(prepared.tool_less_instructions_path.is_some());
    assert_eq!(prepared.context_files.unwrap().read.len(), 2);
    std::fs::remove_file(dir.path().join("read.md")).unwrap();
    assert!(prepare_resume_context(
        &params,
        None,
        true,
        crate::agent::provider::SystemPromptDelivery::External,
        capability,
        |_, _| panic!("invalid context must refuse before materialization"),
    )
    .is_none());
}
