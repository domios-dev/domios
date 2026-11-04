use super::*;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

fn context(root: &Path) -> RuntimeContext {
    RuntimeContext {
        tab_id: uuid::Uuid::new_v4().to_string(),
        epoch: 3,
        binding_nonce: "own-nonce".into(),
        thread_id: Some("own-thread".into()),
        active_turn_id: Some("own-turn".into()),
        cwd: root.to_string_lossy().into_owned(),
        ..Default::default()
    }
}
fn capabilities() -> RuntimeClientBridges {
    RuntimeClientBridges {
        fs_read: true,
        fs_write: true,
        terminal: true,
    }
}
fn root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap();
    (temp, path)
}

#[test]
fn traversal_outside_network_ads_control_and_long_paths_are_denied() {
    let (_temp, root) = root();
    for path in [
        root.join("../escape").to_string_lossy().into_owned(),
        format!("{}-other/file", root.display()),
        "relative".into(),
        "//server/share/file".into(),
        "\\\\server\\share\\file".into(),
        "C:relative".into(),
        format!("{}/nul\0path", root.display()),
        format!("{}/file:stream", root.display()),
        format!("{}/bad\npath", root.display()),
        format!("{}/{}", root.display(), "x".repeat(fs::MAX_PATH)),
    ] {
        assert!(fs::relative(&root, &path).is_err(), "{path:?}");
    }
    assert_eq!(
        fs::relative(&root, root.join("file").to_str().unwrap()).unwrap(),
        Path::new("file")
    );
}

#[cfg(unix)]
#[test]
fn symlink_in_root_and_escaping_and_nonregular_files_are_denied() {
    let (_temp, root) = root();
    let (_outside, outside) = self::root();
    std::fs::write(root.join("regular"), "inside").unwrap();
    std::fs::write(outside.join("regular"), "outside").unwrap();
    std::os::unix::fs::symlink(root.join("regular"), root.join("internal-link")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("external-link")).unwrap();
    let handle = codeterm_blueprint_types::code_source::open_directory_no_follow(&root).unwrap();
    std::os::unix::fs::symlink(&root, root.join("self-link")).unwrap();
    for path in [
        "internal-link",
        "external-link/regular",
        "self-link/regular",
    ] {
        assert!(fs::read(&root, &handle, Path::new(path), None, None).is_err());
        assert!(
            fs::write(&root, &handle, Path::new(path), "replace", |commit| commit(
            ))
            .is_err()
        );
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("regular")).unwrap(),
        "outside"
    );
    std::fs::create_dir(root.join("directory")).unwrap();
    assert!(fs::read(&root, &handle, Path::new("directory"), None, None).is_err());
    assert!(fs::write(
        &root,
        &handle,
        Path::new("directory"),
        "replace",
        |commit| commit()
    )
    .is_err());
    let fifo = std::ffi::CString::new(root.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(fs::read(&root, &handle, Path::new("pipe"), None, None).is_err());
    assert_eq!(
        fs::read(&root, &handle, Path::new("regular"), None, None).unwrap(),
        "inside"
    );
}

#[cfg(unix)]
#[test]
fn oversized_reads_and_writes_refused_and_lines_honored() {
    let (_temp, root) = root();
    let handle = codeterm_blueprint_types::code_source::open_directory_no_follow(&root).unwrap();
    std::fs::write(root.join("large"), vec![b'x'; fs::MAX_BYTES + 1]).unwrap();
    assert!(fs::read(&root, &handle, Path::new("large"), None, None).is_err());
    assert!(fs::write(
        &root,
        &handle,
        Path::new("new"),
        &"x".repeat(fs::MAX_BYTES + 1),
        |commit| commit()
    )
    .is_err());
    assert!(!root.join("new").exists());
    std::fs::write(root.join("lines"), "one\ntwo\nthree").unwrap();
    assert!(fs::read(&root, &handle, Path::new("lines"), Some(0), None).is_err());
    assert_eq!(
        fs::read(&root, &handle, Path::new("lines"), Some(2), Some(1)).unwrap(),
        "two\n"
    );
    assert_eq!(
        fs::read(&root, &handle, Path::new("lines"), Some(2), Some(0)).unwrap(),
        ""
    );
}

#[cfg(unix)]
#[test]
fn atomic_write_preserves_open_old_file_and_rejects_replaced_root() {
    use std::io::Read;
    let (_temp, root) = root();
    let handle = codeterm_blueprint_types::code_source::open_directory_no_follow(&root).unwrap();
    std::fs::write(root.join("file"), "old").unwrap();
    let mut old = std::fs::File::open(root.join("file")).unwrap();
    fs::write(&root, &handle, Path::new("file"), "new", |commit| commit()).unwrap();
    let mut previous = String::new();
    old.read_to_string(&mut previous).unwrap();
    assert_eq!(previous, "old");
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "new");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    let moved = root.with_extension("retired");
    std::fs::rename(&root, &moved).unwrap();
    std::fs::create_dir(&root).unwrap();
    assert!(
        fs::write(&root, &handle, Path::new("file"), "escaped", |commit| {
            commit()
        })
        .is_err()
    );
    std::fs::remove_dir_all(moved).unwrap();
}

#[test]
fn secret_path_classification_separates_machine_stores() {
    for path in [
        ".env",
        ".env.local",
        "key.pem",
        "id_rsa.backup",
        "credentials.json",
        "token-store",
    ] {
        assert!(fs::secret(Path::new(path)));
    }
    assert!(fs::machine_secret(Path::new(".ssh/id_rsa")));
    assert!(!fs::secret(Path::new("src/main.rs")));
}

#[test]
fn terminal_environment_refuses_secrets_and_strips_inherited_credentials() {
    let base = HashMap::from([
        ("PATH".into(), "/safe/bin".into()),
        ("OPENAI_API_KEY".into(), "secret".into()),
        ("CODETERM_API_TOKEN".into(), "secret".into()),
    ]);
    let denied = [terminal::Env {
        name: "TOKEN".into(),
        value: "secret".into(),
    }];
    assert!(terminal::environment(base.clone(), &denied, &[]).is_err());
    let env = terminal::environment(base, &[], &["PATH".into()]).unwrap();
    assert!(!env.contains_key("OPENAI_API_KEY"));
    assert!(!env.contains_key("CODETERM_API_TOKEN"));
    assert!(!env.contains_key("PATH"));
}

#[cfg(unix)]
#[test]
fn foreign_session_epoch_and_nonce_refused_before_approval() {
    let (_temp, root) = root();
    let ctx = context(&root);
    install(&ctx, Some(capabilities())).unwrap();
    for foreign in [
        RuntimeContext {
            epoch: 4,
            ..ctx.clone()
        },
        RuntimeContext {
            binding_nonce: "foreign".into(),
            ..ctx.clone()
        },
    ] {
        assert!(bridge(&foreign).is_err());
    }
    let installed = bridge(&ctx).unwrap();
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    let foreign = Request::Read {
        session_id: "foreign".into(),
        path: root.join("file").to_string_lossy().into_owned(),
        line: None,
        limit: None,
    };
    assert!(prepare(&fixture.service, &session, &ctx, &installed, &foreign).is_err());
    BRIDGES.lock().remove(&ctx.tab_id);
}

#[cfg(unix)]
#[test]
fn secret_read_asks_oversized_write_never_asks_and_unapproved_write_refused() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let bridge = bridge(&context).unwrap();
        let process = session.process.lock().clone().unwrap();
        std::fs::write(root.join(".env.local"), "workspace-secret").unwrap();
        let read = Request::Read {
            session_id: context.thread_id.clone().unwrap(),
            path: root.join(".env.local").to_string_lossy().into_owned(),
            line: None,
            limit: None,
        };
        assert!(
            prepare(&fixture.service, &session, &context, &bridge, &read)
                .unwrap()
                .0
                .is_some()
        );
        let write = Request::Write {
            session_id: context.thread_id.clone().unwrap(),
            path: root.join("new").to_string_lossy().into_owned(),
            content: "x".repeat(fs::MAX_BYTES + 1),
        };
        assert!(prepare(&fixture.service, &session, &context, &bridge, &write).is_err());
        let id = RpcId::Number(18);
        assert!(!answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()]
            }
        )
        .await
        .unwrap());
        assert!(!root.join("new").exists());
        let params =
            json!({"sessionId":context.thread_id,"path":root.join("new"),"content":"approved"});
        assert!(handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/write_text_file",
            &params
        )
        .await
        .unwrap());
        assert!(!root.join("new").exists());
        assert!(answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["reject_once".into()]
            }
        )
        .await
        .unwrap());
        assert!(!root.join("new").exists());
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(serde_json::from_str::<Value>(&line)
            .unwrap()
            .get("error")
            .is_some());
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(unix)]
#[test]
fn approved_write_commits_once_and_expired_turn_cannot_commit() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        let id = RpcId::Number(22);
        let params =
            json!({"sessionId":context.thread_id,"path":root.join("file"),"content":"committed"});
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/write_text_file",
            &params,
        )
        .await
        .unwrap();
        session.context.lock().active_turn_id = Some("next-turn".into());
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        assert!(!root.join("file").exists());
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/write_text_file",
            &params,
        )
        .await
        .unwrap();
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "committed"
        );
        assert!(!answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()]
            }
        )
        .await
        .unwrap());
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(serde_json::from_str::<Value>(&line)
            .unwrap()
            .get("error")
            .is_some());
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["result"],
            json!({})
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(unix)]
#[test]
fn terminal_argv_mismatch_is_refused_without_spawning() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        let id = RpcId::Number(24);
        let params = json!({"sessionId":context.thread_id,"command":"/usr/bin/touch","args":[root.join("unexpected")]});
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "terminal/create",
            &params,
        )
        .await
        .unwrap();
        session.prompts.lock().get_mut(&id).unwrap().request.params["args"] =
            json!([root.join("changed")]);
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        assert!(!root.join("unexpected").exists());
        assert!(!root.join("changed").exists());
        assert!(bridge(&context).unwrap().state.lock().terminals.is_empty());
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(
            serde_json::from_str::<Value>(&line).unwrap()["error"]["message"]
                .as_str()
                .unwrap()
                .contains("changed")
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(unix)]
async fn live_terminal(
    fixture: &crate::service::test_fixture::ServiceFixture,
    session: &Arc<Session>,
    process: &Arc<Process>,
    peer: tokio::io::DuplexStream,
    root: &Path,
) -> (Arc<terminal::Terminal>, BufReader<tokio::io::DuplexStream>) {
    let context = session.context.lock().clone();
    std::fs::write(root.join("output"), vec![b'x'; fs::MAX_BYTES + 8192]).unwrap();
    let id = RpcId::Number(30);
    let params = json!({"sessionId":context.thread_id,"command":"/usr/bin/tail","args":["-c","+1","-f",root.join("output").to_str().unwrap()],"outputByteLimit":fs::MAX_BYTES});
    handle(
        &fixture.service,
        session,
        process,
        &id,
        "terminal/create",
        &params,
    )
    .await
    .unwrap();
    assert!(bridge(&context).unwrap().state.lock().terminals.is_empty());
    answer(
        &fixture.service,
        session,
        process,
        &id,
        &PromptAnswer::Choice {
            values: vec!["allow_once".into()],
        },
    )
    .await
    .unwrap();
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    let terminal_id = reply["result"]["terminalId"].as_str().unwrap();
    let terminal = bridge(&context)
        .unwrap()
        .state
        .lock()
        .terminals
        .get(terminal_id)
        .unwrap()
        .clone();
    let mut changes = terminal.changes.clone();
    loop {
        changes.borrow_and_update();
        if terminal.output()["truncated"] == true {
            break;
        }
        changes.changed().await.unwrap();
    }
    assert_eq!(
        terminal.output()["output"].as_str().unwrap().len(),
        fs::MAX_BYTES
    );
    assert!(terminal.output()["exitStatus"].is_null());
    (terminal, reader)
}

#[cfg(unix)]
#[test]
fn terminal_killed_on_cancel_with_exit_evidence_and_handles_released() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        let (terminal, _reader) = live_terminal(&fixture, &session, &process, peer, &root).await;
        cancel(&fixture.service, &session, &process, false)
            .await
            .unwrap();
        assert!(terminal.wait().await.unwrap()["signal"].is_string());
        assert!(bridge(&context).unwrap().state.lock().terminals.is_empty());
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
        assert!(bridge(&context).is_err());
    });
}

#[cfg(unix)]
#[test]
fn terminal_killed_on_tab_close_with_exit_evidence() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        let (terminal, _reader) = live_terminal(&fixture, &session, &process, peer, &root).await;
        fixture.service.close_tab_async(&tab).await.unwrap();
        assert!(terminal.wait().await.unwrap()["signal"].is_string());
        assert!(bridge(&context).is_err());
    });
}

#[cfg(unix)]
#[test]
fn atomic_commit_rechecks_parent_after_approval_without_following_replacement_link() {
    let (_temp, root) = root();
    let (_outside, outside) = self::root();
    std::fs::create_dir(root.join("parent")).unwrap();
    std::fs::write(root.join("parent/file"), "old").unwrap();
    std::fs::write(outside.join("file"), "outside").unwrap();
    let handle = codeterm_blueprint_types::code_source::open_directory_no_follow(&root).unwrap();
    let result = fs::write(&root, &handle, Path::new("parent/file"), "new", |commit| {
        std::fs::rename(root.join("parent"), root.join("retired")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("parent")).unwrap();
        commit()
    });
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("file")).unwrap(),
        "outside"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("retired/file")).unwrap(),
        "old"
    );
    assert_eq!(std::fs::read_dir(root.join("retired")).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn secret_read_returns_content_only_after_its_scoped_approval() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut ctx = session.context.lock();
            ctx.cwd = root.to_string_lossy().into_owned();
            ctx.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        std::fs::write(root.join(".env.local"), "workspace-only").unwrap();
        let id = RpcId::Number(40);
        let params = json!({"sessionId":context.thread_id,"path":root.join(".env.local")});
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/read_text_file",
            &params,
        )
        .await
        .unwrap();
        assert_eq!(bridge(&context).unwrap().state.lock().pending.len(), 1);
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["reject_once".into()],
            },
        )
        .await
        .unwrap();
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/read_text_file",
            &params,
        )
        .await
        .unwrap();
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(serde_json::from_str::<Value>(&line)
            .unwrap()
            .get("error")
            .is_some());
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["result"],
            json!({"content":"workspace-only"})
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(not(unix))]
#[test]
fn unavailable_platform_installs_no_bridge_or_host_powers() {
    let (_temp, root) = root();
    let context = context(&root);
    assert_eq!(install(&context, Some(capabilities())).unwrap(), None);
    assert!(bridge(&context).is_err());
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn owner_approval_async_preserves_scope_without_entering_a_nested_runtime() {
    use crate::service::tab::RespondPromptOutcome;
    let (_temp, root) = root();
    let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    {
        let mut context = session.context.lock();
        context.cwd = root.to_string_lossy().into_owned();
        context.active_turn_id = Some("owner-turn".into());
    }
    let context = session.context.lock().clone();
    install(&context, Some(capabilities())).unwrap();
    let process = session.process.lock().clone().unwrap();
    let id = RpcId::Number(51);
    let params = json!({"sessionId":context.thread_id,"path":root.join("approved"),"content":"owner-approved"});
    handle(
        &fixture.service,
        &session,
        &process,
        &id,
        "fs/write_text_file",
        &params,
    )
    .await
    .unwrap();
    assert_eq!(
        fixture
            .service
            .respond_prompt_async(&tab, "foreign", None)
            .await
            .unwrap(),
        RespondPromptOutcome::InvalidOption
    );
    assert_eq!(
        fixture.service.respond_prompt(&tab, "foreign").unwrap(),
        RespondPromptOutcome::InvalidOption
    );
    assert!(!root.join("approved").exists());
    assert_eq!(
        fixture
            .service
            .respond_prompt_async(&tab, "allow_once", None)
            .await
            .unwrap(),
        RespondPromptOutcome::Structured
    );
    assert_eq!(
        std::fs::read_to_string(root.join("approved")).unwrap(),
        "owner-approved"
    );
    assert_eq!(
        fixture
            .service
            .respond_prompt_async(&tab, "allow_once", None)
            .await
            .unwrap(),
        RespondPromptOutcome::StalePrompt
    );
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["result"],
        json!({})
    );
    cancel(&fixture.service, &session, &process, true)
        .await
        .unwrap();
}
