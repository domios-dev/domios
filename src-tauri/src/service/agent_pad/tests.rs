use super::*;

#[test]
fn role_seeds_are_created_once_and_nested_utf8_is_exact() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-full-123", AgentPadRole::Worker).unwrap();
    assert!(pad.path().join("message.md").is_file());
    assert!(pad.path().join("notes.md").is_file());
    assert!(pad.path().join("report.md").is_file());
    fs::remove_file(pad.path().join("notes.md")).unwrap();
    fs::create_dir(pad.path().join("nested")).unwrap();
    fs::write(pad.path().join("nested/report.md"), "Привіт\n`$`\n").unwrap();
    assert_eq!(
        pad.read_utf8(Path::new("nested/report.md")).unwrap(),
        "Привіт\n`$`\n"
    );
    assert!(!pad.path().join("notes.md").exists());
}

#[test]
fn unsafe_paths_and_sibling_access_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-a", AgentPadRole::General).unwrap();
    let sibling = AgentPad::create(temp.path(), "tab-b", AgentPadRole::General).unwrap();
    fs::write(sibling.path().join("secret.md"), "secret").unwrap();
    assert!(pad.read_utf8(Path::new("../tab-b/secret.md")).is_err());
    assert!(pad.read_utf8(pad.path()).is_err());
    assert!(pad.read_utf8(Path::new("notes.md/more")).is_err());
}

#[test]
fn revoke_is_exact_and_reconciliation_preserves_only_live_tabs() {
    let temp = tempfile::tempdir().unwrap();
    let live = AgentPad::create(temp.path(), "tab-live", AgentPadRole::Planner).unwrap();
    let gone = AgentPad::create(temp.path(), "tab-gone", AgentPadRole::Orchestrator).unwrap();
    gone.revoke().unwrap();
    assert!(!temp
        .path()
        .join("runtime")
        .join("agent-pads")
        .join("tab-gone")
        .exists());
    let orphan = AgentPad::create(temp.path(), "tab-orphan", AgentPadRole::General).unwrap();
    drop(orphan);
    reconcile(temp.path(), &HashSet::from(["tab-live".to_string()])).unwrap();
    assert!(live.path().is_dir());
    assert!(!temp
        .path()
        .join("runtime")
        .join("agent-pads")
        .join("tab-orphan")
        .exists());
    revoke(&temp.path().join("runtime").join("agent-pads"), "tab-live").unwrap();
    revoke(&temp.path().join("runtime").join("agent-pads"), "tab-live").unwrap();
}

#[test]
fn instances_and_full_tab_ids_have_disjoint_roots() {
    let temp = tempfile::tempdir().unwrap();
    let prod = AgentPad::create(
        &temp.path().join("prod"),
        "tab-full-a",
        AgentPadRole::General,
    )
    .unwrap();
    let dev = AgentPad::create(
        &temp.path().join("dev"),
        "tab-full-a",
        AgentPadRole::General,
    )
    .unwrap();
    let sibling = AgentPad::create(
        &temp.path().join("prod"),
        "tab-full-b",
        AgentPadRole::General,
    )
    .unwrap();
    assert_ne!(prod.path(), dev.path());
    assert_ne!(prod.path(), sibling.path());
    assert!(AgentPad::open(&temp.path().join("prod"), "tab-full-a").is_ok());
    assert!(AgentPad::open(&temp.path().join("prod"), "tab-full").is_err());
}

#[test]
fn coordinator_and_worker_seeds_are_role_specific() {
    let temp = tempfile::tempdir().unwrap();
    let coordinator =
        AgentPad::create(temp.path(), "coordinator", AgentPadRole::Orchestrator).unwrap();
    let worker = AgentPad::create(temp.path(), "worker", AgentPadRole::Worker).unwrap();
    assert!(coordinator.path().join("dispatch.md").is_file());
    assert!(!coordinator.path().join("report.md").exists());
    assert!(worker.path().join("report.md").is_file());
    assert!(!worker.path().join("dispatch.md").exists());
}

#[test]
fn list_is_relative_sorted_and_regular_files_only() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-list", AgentPadRole::General).unwrap();
    fs::create_dir(pad.path().join("nested")).unwrap();
    fs::write(pad.path().join("nested/z.md"), "z").unwrap();
    let files = pad.list().unwrap();
    assert!(files.iter().all(|path| path.is_relative()));
    assert!(files.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(files.contains(&PathBuf::from("nested/z.md")));
}

#[test]
fn reconciliation_uses_id_not_role_title_task_or_cwd() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        AgentPad::create(temp.path(), "canonical-live-id", AgentPadRole::Worker).unwrap();
    let collision = AgentPad::create(
        temp.path(),
        "same-title-role-task-cwd",
        AgentPadRole::Worker,
    )
    .unwrap();
    reconcile(
        temp.path(),
        &HashSet::from(["canonical-live-id".to_string()]),
    )
    .unwrap();
    assert!(retained.path().exists());
    assert!(!collision.path().exists());
}

#[test]
fn provider_restart_retains_pad_and_final_close_revokes_it() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "restartable-tab", AgentPadRole::Worker).unwrap();
    fs::write(pad.path().join("report.md"), "durable while live").unwrap();
    drop(pad);
    let reopened = AgentPad::open(temp.path(), "restartable-tab").unwrap();
    assert_eq!(
        reopened.read_utf8(Path::new("report.md")).unwrap(),
        "durable while live"
    );
    reopened.revoke().unwrap();
    assert!(AgentPad::open(temp.path(), "restartable-tab").is_err());
}

#[test]
fn restore_reuses_existing_pad_and_creates_only_when_missing() {
    let temp = tempfile::tempdir().unwrap();
    let existing = AgentPad::create(temp.path(), "existing-tab", AgentPadRole::Worker).unwrap();
    fs::write(existing.path().join("report.md"), "preserved").unwrap();

    let restored = restore(temp.path(), "existing-tab", AgentPadRole::General).unwrap();
    assert_eq!(
        restored.read_utf8(Path::new("report.md")).unwrap(),
        "preserved"
    );

    let created = restore(temp.path(), "missing-tab", AgentPadRole::Orchestrator).unwrap();
    assert!(created.path().join("dispatch.md").is_file());
}

#[test]
fn startup_reconciliation_removes_prior_tombstones() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime").join("agent-pads");
    fs::create_dir_all(root.join(".revoked-tab-old-token")).unwrap();
    fs::write(root.join(".revoked-tab-old-token/report.md"), "old").unwrap();
    reconcile(temp.path(), &HashSet::new()).unwrap();
    assert!(!root.join(".revoked-tab-old-token").exists());
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-safe", AgentPadRole::General).unwrap();
    let outside = temp.path().join("outside.md");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, pad.path().join("escape.md")).unwrap();
    assert!(pad.read_utf8(Path::new("escape.md")).is_err());
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_paths_are_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-safe", AgentPadRole::General).unwrap();
    let invalid = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
    fs::write(pad.path().join(invalid), b"x").unwrap();
    assert!(pad.list().is_err());
}

#[test]
fn long_body_to_paste_limited_provider_goes_via_recipient_pad() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "recipient-tab", AgentPadRole::General).unwrap();
    let body = "Привіт\nsecond line\n\0sentinel".as_bytes();
    let first = pad.write_inbox_body(body).unwrap();
    let second = pad.write_inbox_body(body).unwrap();
    assert_ne!(first, second);
    assert_eq!(fs::read(first).unwrap(), body);
    assert_eq!(fs::read(second).unwrap(), body);
}

#[test]
fn substitution_pointer_is_single_line_and_carries_the_supplied_path() {
    let supplied = PathBuf::from("C:/agent-pad/inbox/token.md");
    let path_token = supplied.to_str().unwrap().to_string();
    let body = "x".repeat(4097);
    let (delivered, path) =
        super::substitute_body_with_limit(body, 4097, Some(4096), |_| Ok(supplied.clone()))
            .unwrap();
    assert_eq!(path.as_deref(), Some(supplied.as_path()));
    assert_eq!(delivered.lines().count(), 1);
    assert!(delivered.contains(&path_token));
}

#[test]
fn short_body_is_sent_unchanged() {
    let body = "under limit".to_string();
    let (delivered, path) =
        super::substitute_body_with_limit(body.clone(), body.len(), Some(4096), |_| {
            panic!("short payload must not write an inbox file")
        })
        .unwrap();
    assert_eq!(delivered, body);
    assert_eq!(path, None);
}

#[test]
fn providers_without_paste_limit_are_unchanged() {
    let body = "x".repeat(8193);
    let (delivered, path) =
        super::substitute_body_with_limit(body.clone(), body.len(), None, |_| {
            panic!("undeclared capability must not write an inbox file")
        })
        .unwrap();
    assert_eq!(delivered, body);
    assert_eq!(path, None);
}

#[test]
fn paste_limit_counts_utf8_bytes() {
    let body = "Ж".repeat(2049);
    let (delivered, path) = super::substitute_body_with_limit(body, 4098, Some(4096), |bytes| {
        assert_eq!(bytes.len(), 4098);
        Ok(PathBuf::from("recipient/inbox/body.md"))
    })
    .unwrap();
    assert!(path.is_some());
    assert_eq!(delivered.lines().count(), 1);
}

#[test]
fn pad_write_failure_refuses_the_send() {
    let body = "x".repeat(4097);
    let result = super::substitute_body_with_limit(body.clone(), body.len(), Some(4096), |_| {
        Err("disk unavailable".into())
    });
    assert!(result.is_err());
}

#[test]
fn enveloped_limit_boundary_measures_final_paste_bytes() {
    let supplied = PathBuf::from("pad/inbox/token.md");
    let envelope_bytes = 200;
    let body = "x".repeat(4096 - envelope_bytes + 1);
    let (delivered, path) = super::substitute_body_with_limit(
        body.clone(),
        body.len() + envelope_bytes,
        Some(4096),
        |bytes| {
            assert_eq!(bytes, body.as_bytes());
            Ok(supplied.clone())
        },
    )
    .unwrap();
    assert_eq!(path, Some(supplied.clone()));
    assert!(delivered.contains(supplied.to_str().unwrap()));
}

#[cfg(windows)]
#[test]
fn chat_send_and_gateway_deliveries_substitute_too() {
    use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
    use crate::service::{
        messenger::{MessageEnvelope, Messenger},
        test_fixture::ServiceFixture,
    };

    let (mut mux, sender) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&sender).unwrap();
    let recipient = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&recipient, AgentRole::Worker, Some(&sender))
        .unwrap();
    mux.get_tab_mut(&recipient).unwrap().agent_type = Some("limited-test-provider".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("limited-test-provider")
            .with_paste_limit_bytes(4096),
    ));

    let chat_body = "chat content\nend marker".repeat(400);
    let (chat_pointer, chat_file) = fixture
        .service
        .prepare_chat_send_text(&recipient, chat_body.clone(), false)
        .unwrap();
    let chat_file = chat_file.unwrap();
    assert_eq!(fs::read(&chat_file).unwrap(), chat_body.as_bytes());
    assert!(chat_pointer.contains(chat_file.to_str().unwrap()));
    assert_eq!(chat_pointer.lines().count(), 1);

    let gateway_body = "gateway content\nend marker".repeat(400);
    let envelope = MessageEnvelope::tab("Sender", sender);
    let (delivery, gateway_file) =
        Messenger::deliver_with_pad_file(&fixture.service, &recipient, envelope, &gateway_body)
            .unwrap();
    let gateway_file = gateway_file.unwrap();
    assert_eq!(fs::read(&gateway_file).unwrap(), gateway_body.as_bytes());
    let crate::service::messenger::Delivery::Pty { text } = delivery else {
        panic!("PTY delivery expected")
    };
    assert!(text.contains(gateway_file.to_str().unwrap()));
}

#[cfg(windows)]
#[test]
fn mesh_received_long_send_lands_in_local_recipient_pad() {
    use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
    use crate::service::{
        messenger::{MessageEnvelope, Messenger},
        test_fixture::ServiceFixture,
    };

    let (mut mux, sender) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&sender).unwrap();
    let recipient = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&recipient, AgentRole::Worker, Some(&sender))
        .unwrap();
    mux.get_tab_mut(&recipient).unwrap().agent_type = Some("limited-test-provider".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("limited-test-provider")
            .with_paste_limit_bytes(4096),
    ));
    let original = "mesh payload\nend sentinel".repeat(400);
    let envelope = MessageEnvelope::tab("Remote", sender.clone()).mesh(Some("peer-token".into()));
    let delivery =
        Messenger::deliver(&fixture.service, &recipient, envelope.clone(), &original).unwrap();
    let pad = AgentPad::open(fixture.temp.path(), &recipient).unwrap();
    let files = pad.list().unwrap();
    let inbox_file = files.iter().find(|file| file.starts_with("inbox")).unwrap();
    let path_token = pad.path().join(inbox_file).to_str().unwrap().to_string();
    let crate::service::messenger::Delivery::Pty { text } = delivery else {
        panic!("PTY delivery expected")
    };
    assert_eq!(
        pad.read_utf8(inbox_file).unwrap().as_bytes(),
        original.as_bytes()
    );
    assert!(text.contains(&path_token));
    assert!(text.contains(&crate::service::messenger::Messenger::pty_prefix(&envelope)));
    assert!(text.contains("peer-token"));
}
