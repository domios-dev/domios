use super::*;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

pub(super) fn context(root: &Path) -> RuntimeContext {
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
pub(super) fn capabilities() -> RuntimeClientBridges {
    RuntimeClientBridges {
        fs_read: true,
        fs_write: true,
        terminal: true,
    }
}
pub(super) fn root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap();
    (temp, path)
}

fn open_root(root: &Path) -> std::fs::File {
    #[cfg(unix)]
    {
        codeterm_blueprint_types::code_source::open_directory_no_follow(root).unwrap()
    }
    #[cfg(windows)]
    {
        fs::windows::open_root(root).unwrap()
    }
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
    let handle = open_root(&root);
    std::os::unix::fs::symlink(&root, root.join("self-link")).unwrap();
    for path in [
        "internal-link",
        "external-link/regular",
        "self-link/regular",
    ] {
        assert_eq!(
            bridge_failure(fs::read(&root, &handle, Path::new(path), None, None).unwrap_err()).code,
            -32602,
        );
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

#[cfg(any(unix, windows))]
#[test]
fn oversized_reads_and_writes_refused_and_lines_honored() {
    let (_temp, root) = root();
    let handle = open_root(&root);
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
    let handle = open_root(&root);
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

#[test]
fn terminal_children_keep_host_launch_identity_and_refuse_agent_overrides() {
    let base = HashMap::from([
        (
            "CODETERM_LAUNCH_FILE".into(),
            "/private/own-launch.json".into(),
        ),
        ("CODETERM_TAB_ID".into(), "own-tab".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "own-nonce".into()),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:17686".into()),
        ("CODETERM_API_TOKEN".into(), "own-token".into()),
    ]);
    let env = terminal::environment(base.clone(), &[], &["CODETERM_API_TOKEN".into()]).unwrap();
    for key in [
        "CODETERM_LAUNCH_FILE",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_API_URL",
    ] {
        assert_eq!(env[key], base[key]);
        assert!(terminal::environment(
            base.clone(),
            &[terminal::Env {
                name: key.into(),
                value: "foreign".into(),
            }],
            &[]
        )
        .is_err());
    }
    assert!(!env.contains_key("CODETERM_API_TOKEN"));
}

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
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
    #[cfg(unix)]
    let params = json!({"sessionId":context.thread_id,"command":"/usr/bin/tail","args":["-c","+1","-f",root.join("output").to_str().unwrap()],"outputByteLimit":fs::MAX_BYTES});
    #[cfg(windows)]
    let params = json!({"sessionId":context.thread_id,"command":std::env::current_exe().unwrap(),"args":["--exact","service::agent_runtime::client_bridge::windows_tests::windows_terminal_process_fixture","--ignored","--nocapture","--test-threads=1"],"outputByteLimit":fs::MAX_BYTES});
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

#[cfg(any(unix, windows))]
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
        #[cfg(windows)]
        let descendant = super::windows_tests::retain_descendant(&root);
        cancel(&fixture.service, &session, &process, false)
            .await
            .unwrap();
        let status = terminal.wait().await.unwrap();
        #[cfg(unix)]
        assert!(status["signal"].is_string());
        #[cfg(windows)]
        {
            assert!(status["exitCode"].is_number());
            super::windows_tests::assert_descendant_exited(&descendant);
        }
        assert!(bridge(&context).unwrap().state.lock().terminals.is_empty());
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
        assert!(bridge(&context).is_err());
    });
}

#[cfg(any(unix, windows))]
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
        #[cfg(windows)]
        let descendant = super::windows_tests::retain_descendant(&root);
        fixture.service.close_tab_async(&tab).await.unwrap();
        let status = terminal.wait().await.unwrap();
        #[cfg(unix)]
        assert!(status["signal"].is_string());
        #[cfg(windows)]
        {
            assert!(status["exitCode"].is_number());
            super::windows_tests::assert_descendant_exited(&descendant);
        }
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
    let handle = open_root(&root);
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

#[cfg(any(unix, windows))]
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

#[cfg(not(any(unix, windows)))]
#[test]
fn unavailable_platform_installs_no_bridge_or_host_powers() {
    let (_temp, root) = root();
    let context = context(&root);
    assert_eq!(install(&context, Some(capabilities())).unwrap(), None);
    assert!(bridge(&context).is_err());
}

#[cfg(any(unix, windows))]
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

#[test]
fn client_refusal_rpc_data_carries_provider_neutral_reason_without_changing_message() {
    for (message, reason) in [
        ("file_in_use: File is in use", "file_in_use"),
        (
            "file_locked: File has an incompatible open handle",
            "file_locked",
        ),
        (
            "atomic_replace_unsupported: Native replacement unavailable",
            "atomic_replace_unsupported",
        ),
        ("Reparse attribute traversal denied", "reparse_point"),
        ("Client bridge capacity exceeded", "client_bridge_refused"),
    ] {
        assert_eq!(
            serde_json::to_value(failure(message.into())).unwrap(),
            json!({"code":-32602,"message":message,"data":{"reason":reason}})
        );
    }
}

#[test]
fn write_commit_releases_context_and_reports_committed_supersession_honestly() {
    let (_temp, root) = root();
    let expected = context(&root);
    let current = Mutex::new(expected.clone());
    let path = root.join("committed");
    current.lock().active_turn_id = Some("foreign".into());
    assert!(commit_scoped_write(&current, &expected, &mut || {
        panic!("expired grant cannot commit")
    })
    .is_err());
    assert!(!path.exists());
    *current.lock() = expected.clone();
    let message = commit_scoped_write(&current, &expected, &mut || {
        let mut guard = current
            .try_lock()
            .expect("context must be unlocked during filesystem commit");
        std::fs::write(&path, "written").unwrap();
        guard.active_turn_id = Some("superseded".into());
        Ok(())
    })
    .unwrap_err();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "written");
    let error = failure(message.clone());
    assert_eq!(error.message, message);
    assert!(error.message.contains("was written"));
    assert_eq!(
        error.data,
        Some(json!({"reason":"write_committed_superseded","committed":true}))
    );
    *current.lock() = expected.clone();
    commit_scoped_write(&current, &expected, &mut || {
        assert!(current.try_lock().is_some());
        Ok(())
    })
    .unwrap();
}

#[cfg(unix)]
#[test]
fn superseded_unix_write_syncs_parent_before_reporting_committed() {
    let (_temp, root) = root();
    let handle = open_root(&root);
    let expected = context(&root);
    let current = Mutex::new(expected.clone());
    let syncs = std::cell::Cell::new(0);
    let relative = Path::new("durable");
    let message = fs::write_with_sync(
        &root,
        &handle,
        relative,
        "written",
        |apply| {
            commit_scoped_write(&current, &expected, &mut || {
                apply()?;
                assert_eq!(syncs.get(), 1, "parent fsync must precede supersession");
                current.lock().active_turn_id = Some("superseded".into());
                Ok(())
            })
        },
        |parent| {
            parent.sync_all().map_err(|error| error.to_string())?;
            syncs.set(syncs.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(syncs.get(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join(relative)).unwrap(),
        "written"
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        1,
        "completed rename must leave no temporary entry"
    );
    assert_eq!(
        failure(message).data,
        Some(json!({"reason":"write_committed_superseded","committed":true}))
    );
}

#[cfg(unix)]
#[test]
fn unix_symlink_leaf_and_native_nofollow_errors_report_reparse_point() {
    let (_temp, root) = root();
    std::fs::write(root.join("regular"), "inside").unwrap();
    std::os::unix::fs::symlink(root.join("regular"), root.join("leaf")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("directory-link")).unwrap();
    let handle = open_root(&root);
    for message in [
        fs::read(&root, &handle, Path::new("leaf"), None, None)
            .unwrap_err()
            .to_string(),
        fs::write(&root, &handle, Path::new("leaf"), "blocked", |apply| {
            apply()
        })
        .unwrap_err(),
        fs::open_at_no_follow(
            &handle,
            &std::ffi::CString::new("leaf").unwrap(),
            libc::O_RDONLY | libc::O_NONBLOCK,
        )
        .unwrap_err(),
        fs::directory(&handle, Path::new("directory-link")).unwrap_err(),
    ] {
        assert_eq!(
            failure(message).data,
            Some(json!({"reason":"reparse_point"}))
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("regular")).unwrap(),
        "inside"
    );
    assert_eq!(
        fs::read(&root, &handle, Path::new("regular"), None, None).unwrap(),
        "inside"
    );
}

#[cfg(unix)]
#[test]
fn unix_committed_sync_failure_reports_written_even_when_superseded() {
    for superseded in [true, false] {
        let (_temp, root) = root();
        let handle = open_root(&root);
        let expected = context(&root);
        let current = Mutex::new(expected.clone());
        let message = fs::write_with_sync(
            &root,
            &handle,
            Path::new("unsynced"),
            "written",
            |apply| commit_scoped_write(&current, &expected, apply),
            |_| {
                if superseded {
                    current.lock().active_turn_id = Some("superseded".into());
                }
                Err("injected directory sync failure".into())
            },
        )
        .unwrap_err();
        assert_eq!(
            std::fs::read_to_string(root.join("unsynced")).unwrap(),
            "written"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(message
            .contains("File was written; directory sync failed: injected directory sync failure"));
        assert_eq!(message.contains("turn was superseded"), superseded);
        assert_eq!(
            failure(message).data,
            Some(
                json!({"reason":"write_committed_unsynced","committed":true,"superseded":superseded,"synced":false})
            )
        );
    }
}

#[test]
fn only_typed_missing_leaf_maps_to_resource_not_found() {
    let missing = bridge_failure(BridgeError::NotFound);
    assert_eq!(missing.code, -32002);
    assert_eq!(missing.message, "Resource not found");
    assert_eq!(missing.data, None);
    for message in [
        "Invalid bridge path",
        "Path outside workspace",
        "reparse_point: Symlink traversal denied",
        "Reparse attribute denied",
        "Non-regular file denied",
        "Machine credential store denied",
        "Read denied",
        "Workspace parent unavailable",
    ] {
        let refused = bridge_failure(BridgeError::from(message));
        assert_eq!(refused.code, -32602, "{message}");
        assert_ne!(refused.message, "Resource not found");
    }
}

#[cfg(any(unix, windows))]
#[test]
fn missing_leaf_read_reply_allows_subsequent_approved_creation() {
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
        let path = root.join("created.txt");
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        handle(
            &fixture.service,
            &session,
            &process,
            &RpcId::Number(71),
            "fs/read_text_file",
            &json!({"sessionId":context.thread_id,"path":path}),
        )
        .await
        .unwrap();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":71,"error":{"code":-32002,"message":"Resource not found"}}),
        );
        assert!(!path.exists());
        let installed = bridge(&context).unwrap();
        assert_eq!(
            bridge_failure(
                fs::read(
                    &root,
                    &installed.root_handle,
                    Path::new("created.txt"),
                    None,
                    None
                )
                .unwrap_err()
            )
            .code,
            -32002,
        );
        handle(
            &fixture.service,
            &session,
            &process,
            &RpcId::Number(72),
            "fs/write_text_file",
            &json!({"sessionId":context.thread_id,"path":path,"content":"created through ACP"}),
        )
        .await
        .unwrap();
        assert!(!path.exists());
        answer(
            &fixture.service,
            &session,
            &process,
            &RpcId::Number(72),
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":72,"result":{}}),
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "created through ACP"
        );
        std::fs::create_dir(root.join("directory")).unwrap();
        for denied in [
            root.join("../escape"),
            root.join("directory"),
            root.join(".ssh/absent"),
        ] {
            handle(
                &fixture.service,
                &session,
                &process,
                &RpcId::Number(73),
                "fs/read_text_file",
                &json!({"sessionId":context.thread_id,"path":denied}),
            )
            .await
            .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["error"]["code"],
                -32602
            );
        }
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}
