use super::*;
use crate::js_runtime::vm::PluginVm;
use serde_json::json;

fn vm_with(plugin_src: &str) -> PluginVm {
    PluginVm::from_source("test", plugin_src.to_string()).expect("vm")
}

#[test]
fn sandbox_unrestricted_allows_everything() {
    let sb = PluginSandbox::unrestricted();
    assert!(sb.can_read("/etc/passwd"));
    assert!(sb.can_write("/anywhere"));
    assert!(sb.can_env("AWS_SECRET_ACCESS_KEY"));
}

#[test]
fn sandbox_restricted_scopes_fs_and_blocks_secret_env() {
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("sessions");
    std::fs::create_dir_all(&allowed).unwrap();
    let sb = PluginSandbox::restricted(vec![allowed.clone()], vec![allowed.clone()]);

    let inside = allowed.join("a.jsonl");
    std::fs::write(&inside, "{}").unwrap();
    assert!(sb.can_read(inside.to_str().unwrap()));
    assert!(sb.can_write(allowed.join("b.jsonl").to_str().unwrap()));

    assert!(!sb.can_read("/etc/passwd"));
    assert!(!sb.can_read(allowed.join("../../../../etc/passwd").to_str().unwrap()));
    assert!(!sb.can_write("/tmp/evil"));

    assert!(sb.can_env("XDG_DATA_HOME"));
    assert!(sb.can_env("HOME"));
    assert!(!sb.can_env("CODETERM_API_TOKEN"));
    assert!(!sb.can_env("AWS_SECRET_ACCESS_KEY"));
    assert!(!sb.can_env("ANTHROPIC_API_KEY"));
    assert!(!sb.can_env("GITHUB_PASSWORD"));
}

#[test]
fn granted_cwd_root_widens_reads_only_on_its_own_sandbox() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path().join("ws");
    std::fs::create_dir_all(t.join("a").join(".git")).unwrap();
    let file = t.join("a").join(".git").join("HEAD");
    std::fs::write(&file, "ref: refs/heads/main").unwrap();
    let file_str = file.to_string_lossy().into_owned();

    let granted = PluginSandbox::restricted(vec![], vec![]);
    let bystander = PluginSandbox::restricted(vec![], vec![]);
    assert!(!granted.can_read(&file_str), "no grant yet → denied");

    granted.grant_cwd_read_root(t.to_str().unwrap());
    assert!(granted.can_read(&file_str));
    assert!(
        !bystander.can_read(&file_str),
        "a grant on one sandbox instance must not leak to another"
    );
    assert!(!granted.can_write(&file_str), "cwd grants are read-only");

    // Relative paths are never recorded as roots.
    granted.grant_cwd_read_root("relative/dir");
    assert!(!granted.can_read("relative/dir/x"));

    // Unrestricted sandboxes need no grants and record none.
    let full = PluginSandbox::unrestricted();
    full.grant_cwd_read_root(t.to_str().unwrap());
    assert!(full.granted_read_roots.lock().is_empty());

    #[cfg(windows)]
    {
        // OSC7-style forward-slash cwd must cover native backslash targets.
        let fwd = t.to_string_lossy().replace('\\', "/");
        let sb = PluginSandbox::restricted(vec![], vec![]);
        sb.grant_cwd_read_root(&fwd);
        assert!(
            sb.can_read(&file_str),
            "forward-slash grant covers {file_str}"
        );
    }
}

#[test]
fn granted_cwd_roots_are_bounded_and_lru_refreshed() {
    let tmp = tempfile::tempdir().unwrap();
    let sb = PluginSandbox::restricted(vec![], vec![]);
    for i in 0..=PluginSandbox::MAX_GRANTED_CWD_ROOTS {
        let d = tmp.path().join(format!("d{i}"));
        std::fs::create_dir_all(d.join("inner")).unwrap();
        sb.grant_cwd_read_root(d.to_str().unwrap());
    }
    let first_inner = tmp.path().join("d0").join("inner");
    assert!(
        !sb.can_read(first_inner.to_str().unwrap()),
        "oldest grant is evicted past the cap"
    );
    let last_inner = tmp
        .path()
        .join(format!("d{}", PluginSandbox::MAX_GRANTED_CWD_ROOTS))
        .join("inner");
    assert!(sb.can_read(last_inner.to_str().unwrap()));

    // Re-granting an existing root refreshes it in place, no duplicate entry.
    let last_root = tmp
        .path()
        .join(format!("d{}", PluginSandbox::MAX_GRANTED_CWD_ROOTS));
    let len_before = sb.granted_read_roots.lock().len();
    sb.grant_cwd_read_root(last_root.to_str().unwrap());
    assert_eq!(sb.granted_read_roots.lock().len(), len_before);
}

/// The diagnosis mechanism test: temp tree T/a/.git, T/b/.git, T/c; grant T →
/// `host.listChildDirs(T)` yields a,b,c (dirs only, names only, no recursion)
/// and the `.git` probes pass. Negative controls prove the gate: no grant →
/// denied, ungranted sibling → denied even while T is granted.
#[test]
fn list_child_dirs_gated_by_cwd_grant_with_negative_controls() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path().join("granted-ws");
    std::fs::create_dir_all(t.join("a").join(".git")).unwrap();
    std::fs::create_dir_all(t.join("b").join(".git")).unwrap();
    std::fs::create_dir_all(t.join("c")).unwrap();
    std::fs::write(t.join("not-a-dir.txt"), "x").unwrap();
    let sibling = tmp.path().join("ungranted-ws");
    std::fs::create_dir_all(sibling.join("x").join(".git")).unwrap();

    const SRC: &str = r#"
        module.exports.default = {
            list(p) { return host.listChildDirs(p); },
            exists(p) { return host.fileExists(p); }
        };
    "#;
    let sandbox = Arc::new(PluginSandbox::restricted(vec![], vec![]));
    let vm = PluginVm::from_source_sandboxed("confined", SRC.to_string(), None, sandbox.clone())
        .unwrap();
    let t_str = t.to_string_lossy().into_owned();
    let t_git = t.join("a").join(".git").to_string_lossy().into_owned();

    // NEGATIVE CONTROL: with no grant recorded, listing T is denied and the probe is false.
    let denied: Option<Vec<String>> =
        crate::js_runtime::vm::call_json(&vm, "list", &[json!(t_str)]).unwrap();
    assert_eq!(denied, None, "without a grant the listing must be denied");
    let probe: bool = crate::js_runtime::vm::call_json(&vm, "exists", &[json!(t_git)]).unwrap();
    assert!(!probe, "without a grant the .git probe must be false");

    sandbox.grant_cwd_read_root(&t_str);
    let names: Option<Vec<String>> =
        crate::js_runtime::vm::call_json(&vm, "list", &[json!(t_str)]).unwrap();
    assert_eq!(
        names,
        Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        "child directory names only: no files, no nested .git entries"
    );
    let probe: bool = crate::js_runtime::vm::call_json(&vm, "exists", &[json!(t_git)]).unwrap();
    assert!(probe, "granted cwd root must let the .git probe pass");

    // NEGATIVE CONTROL: ungranted sibling stays denied even while T is granted.
    let sib_str = sibling.to_string_lossy().into_owned();
    let sib_git = sibling
        .join("x")
        .join(".git")
        .to_string_lossy()
        .into_owned();
    let sib: Option<Vec<String>> =
        crate::js_runtime::vm::call_json(&vm, "list", &[json!(sib_str)]).unwrap();
    assert_eq!(sib, None, "ungranted sibling listing must stay denied");
    let sib_probe: bool =
        crate::js_runtime::vm::call_json(&vm, "exists", &[json!(sib_git)]).unwrap();
    assert!(!sib_probe, "ungranted sibling .git probe must stay false");
}

#[test]
fn host_api_v2_gating_by_allowlist() {
    use crate::js_runtime::manifest::{NetworkPermission, PermissionsConfig, SubprocessPermission};
    let perms = PermissionsConfig {
        subprocess: SubprocessPermission {
            allow: vec!["bw".into()],
        },
        network: NetworkPermission {
            allow: vec!["127.0.0.1:7891".into()],
        },
        secrets: true,
        agent_commands: true,
        manage_workspaces: true,
        spawn_agents: true,
        ..Default::default()
    };
    let sb = PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms("myplug", &perms);
    assert_eq!(sb.plugin_id(), "myplug");
    assert!(sb.can_exec("bw"));
    assert!(!sb.can_exec("rm"));
    assert!(sb.can_reach("127.0.0.1:7891"));
    assert!(sb.can_reach("127.0.0.1"));
    assert!(!sb.can_reach("evil.example.com"));
    assert!(sb.secrets_allowed());
    assert!(sb.agent_commands_allowed());
    assert!(sb.manage_workspaces_allowed());
    assert!(sb.spawn_agents_allowed());

    let none = PluginSandbox::restricted(vec![], vec![])
        .with_identity_and_perms("p2", &PermissionsConfig::default());
    assert!(!none.can_exec("bw"));
    assert!(!none.can_reach("127.0.0.1:7891"));
    assert!(!none.secrets_allowed());
    assert!(!none.agent_commands_allowed());
    assert!(!none.manage_workspaces_allowed());
    assert!(!none.spawn_agents_allowed());

    let full =
        PluginSandbox::unrestricted().with_identity_and_perms("sys", &PermissionsConfig::default());
    assert!(full.can_exec("anything"));
    assert!(full.can_reach("anywhere"));
    assert!(full.secrets_allowed());
    assert!(full.agent_commands_allowed());
    assert!(full.manage_workspaces_allowed());
    assert!(full.spawn_agents_allowed());
    assert_eq!(full.plugin_id(), "sys");
}

#[test]
fn diagnostics_that_speak_denial_keeps_calling_plugin_identity() {
    use crate::js_runtime::manifest::PermissionsConfig;
    let sandbox = PluginSandbox::restricted(vec![], vec![])
        .with_identity_and_perms("culprit-plugin", &PermissionsConfig::default());
    assert_eq!(sandbox.plugin_id(), "culprit-plugin");
    assert!(!sandbox.can_exec("ls"));
}

#[test]
fn can_exec_accepts_full_path_by_basename() {
    use crate::js_runtime::manifest::{PermissionsConfig, SubprocessPermission};
    let perms = PermissionsConfig {
        subprocess: SubprocessPermission {
            allow: vec!["ffmpeg".into(), "whisper-cli".into()],
        },
        ..Default::default()
    };
    let sb =
        PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms("transcriber", &perms);

    assert!(sb.can_exec("ffmpeg"));
    assert!(sb.can_exec("whisper-cli"));
    assert!(sb.can_exec("/Users/user/.codeterm/transcriber/bin/ffmpeg"));
    assert!(sb.can_exec("/opt/homebrew/bin/ffmpeg"));
    assert!(sb.can_exec("/Users/user/.codeterm/transcriber/bin/whisper-cli"));
    assert!(!sb.can_exec("rm"));
    assert!(!sb.can_exec("/bin/rm"));
    assert!(!sb.can_exec("/usr/bin/ffmpeg-evil"));
}

#[test]
fn late_ref_resolves_only_after_population_while_target_lives() {
    struct FakeService;
    let r: LateRef<FakeService> = LateRef::new();
    assert!(r.get().is_none(), "unpopulated LateRef must not resolve");

    let svc = Arc::new(FakeService);
    assert!(r.set(Arc::downgrade(&svc)), "first set must succeed");
    let resolved = r.get().expect("populated LateRef must resolve");
    assert!(Arc::ptr_eq(&resolved, &svc));

    let other = Arc::new(FakeService);
    assert!(
        !r.set(Arc::downgrade(&other)),
        "second set must be rejected"
    );
    assert!(Arc::ptr_eq(&r.get().unwrap(), &svc));

    drop(resolved);
    drop(svc);
    assert!(
        r.get().is_none(),
        "LateRef must not resolve a dropped target"
    );
}

#[test]
fn sandbox_service_ref_is_late_bound() {
    struct NullCtx;
    impl crate::host_traits::PluginHostCtx for NullCtx {
        fn mem_search_json(&self, _: &str) -> String {
            String::new()
        }
        fn analytics_query_json(&self, _: &str) -> String {
            String::new()
        }
        fn workspace_ensure_json(&self, _: &str) -> String {
            String::new()
        }
        fn workspace_tabs_json(&self, _: &str) -> String {
            String::new()
        }
        fn close_tab(&self, _: &str) {}
        fn spawn_tab_json(&self, _: &str) -> String {
            String::new()
        }
        fn worker_get_json(&self, _: &str) -> Option<String> {
            None
        }
        fn worker_kill(&self, _: &str) {}
        fn worker_pick_color(&self) -> String {
            String::new()
        }
        fn worker_register_json(&self, _: &str) {}
        fn worker_start_json(&self, _: &str, _: &str) -> String {
            String::new()
        }
        fn worker_poll_json(&self, _: &str) -> String {
            String::new()
        }
        fn agent_spawn_json(&self, _: &str, _: &str, _: &str) -> String {
            String::new()
        }
        fn agent_send_json(&self, _: &str, _: &str, _: &str) -> String {
            String::new()
        }
        fn agent_reap_json(&self, _: &str) -> String {
            String::new()
        }
        fn agent_poll_json(&self, _: &str) -> String {
            String::new()
        }
        fn progress_raw_json(&self, _: &str, _: &str) -> String {
            String::new()
        }
        fn contributions_changed(&self, _: &str) {}
        fn emit_graph_worker_spawned_json(&self, _: &str) {}
        fn socket_display_path(&self) -> String {
            String::new()
        }
        fn api_base_url(&self) -> String {
            String::new()
        }
        fn api_token(&self) -> Option<String> {
            None
        }
        fn secret_get(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn secret_set(&self, _: &str, _: &str, _: &str) -> bool {
            false
        }
        fn secret_delete(&self, _: &str, _: &str) -> bool {
            false
        }
    }

    let sb = PluginSandbox::unrestricted();
    assert!(sb.service().is_none(), "sandbox starts with no service ref");

    // Drop the Arc immediately so the Weak can't upgrade — same semantics as Weak::new()
    let dead: Weak<dyn crate::host_traits::PluginHostCtx> = {
        let a: Arc<dyn crate::host_traits::PluginHostCtx> = Arc::new(NullCtx);
        Arc::downgrade(&a)
    };
    assert!(sb.set_service(dead));
    assert!(sb.service().is_none(), "dead weak ref must resolve to None");

    let dead2: Weak<dyn crate::host_traits::PluginHostCtx> = {
        let a: Arc<dyn crate::host_traits::PluginHostCtx> = Arc::new(NullCtx);
        Arc::downgrade(&a)
    };
    assert!(!sb.set_service(dead2));
}

#[test]
fn host_settings_json_carries_plugin_config() {
    let sb = PluginSandbox::unrestricted().with_settings(r#"{"engine":"local"}"#.to_string());
    assert_eq!(sb.settings_json(), r#"{"engine":"local"}"#);
    assert_eq!(PluginSandbox::unrestricted().settings_json(), "null");
}

#[test]
fn host_platform_matches_native() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                getPlatform() { return host.platform(); }
            };
        "#,
    );
    let p: String = crate::js_runtime::vm::call_json(&vm, "getPlatform", &[]).unwrap();
    assert_eq!(p, std::env::consts::OS);
}

#[test]
fn host_home_dir_returns_path() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                getHome() { return host.homeDir(); }
            };
        "#,
    );
    let p: Option<String> = crate::js_runtime::vm::call_json(&vm, "getHome", &[]).unwrap();
    assert_eq!(
        p,
        dirs::home_dir().map(|p| p.to_string_lossy().into_owned())
    );
}

#[test]
fn host_expand_home_resolves_tilde() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                expand(p) { return host.expandHome(p); }
            };
        "#,
    );
    let p: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "expand", &[json!("~/foo/bar")]).unwrap();
    if let Some(home) = dirs::home_dir() {
        assert_eq!(p, Some(home.join("foo/bar").to_string_lossy().into_owned()));
    }
}

#[test]
fn host_write_then_read_file_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt").to_string_lossy().into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                write(p, c) { return host.writeFile(p, c); },
                read(p) { return host.readFile(p); },
                exists(p) { return host.fileExists(p); }
            };
        "#,
    );
    let ok: bool =
        crate::js_runtime::vm::call_json(&vm, "write", &[json!(path), json!("plugin wrote this")])
            .unwrap();
    assert!(ok);
    let exists: bool = crate::js_runtime::vm::call_json(&vm, "exists", &[json!(path)]).unwrap();
    assert!(exists);
    let body: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "read", &[json!(path)]).unwrap();
    assert_eq!(body.as_deref(), Some("plugin wrote this"));
}

#[test]
fn host_atomic_write_and_exclusive_file_lock_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("config.toml")
        .to_string_lossy()
        .into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                lock(p) { return host.lockFile(p); },
                unlock(id) { return host.unlockFile(id); },
                lockAndUnlock(p) {
                    const id = host.lockFile(p);
                    return id !== null && host.unlockFile(id);
                },
                symlink(p) { return host.fs.isSymlink(p); },
                writeLocked(p, c) {
                    const id = host.lockFile(p);
                    if (id === null) return false;
                    try { return host.writeFileAtomic(p, c); }
                    finally { host.unlockFile(id); }
                }
            };
        "#,
    );
    let lock_id: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "lock", &[json!(path.clone())]).unwrap();
    let lock_id = lock_id.expect("lock acquisition returns an opaque handle");
    let contended: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "lock", &[json!(path.clone())]).unwrap();
    assert!(
        contended.is_some(),
        "a completed plugin call releases leaked lock handles"
    );
    let ok: bool = crate::js_runtime::vm::call_json(
        &vm,
        "writeLocked",
        &[json!(path.clone()), json!("model = \"gpt-5.6\"\n")],
    )
    .unwrap();
    assert!(ok);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "model = \"gpt-5.6\"\n"
    );
    let is_symlink: Option<bool> =
        crate::js_runtime::vm::call_json(&vm, "symlink", &[json!(path.clone())]).unwrap();
    assert_eq!(is_symlink, Some(false));
    let unlocked: bool =
        crate::js_runtime::vm::call_json(&vm, "unlock", &[json!(lock_id)]).unwrap();
    assert!(
        !unlocked,
        "a call-scoped handle cannot outlive the JavaScript call"
    );
    let lock_and_unlock: bool =
        crate::js_runtime::vm::call_json(&vm, "lockAndUnlock", &[json!(path.clone())]).unwrap();
    assert!(
        lock_and_unlock,
        "the normal lock merge remains usable inside one plugin call"
    );
    let external_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    external_lock.try_lock().unwrap();
    let blocked: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "lock", &[json!(path)]).unwrap();
    assert!(
        blocked.is_none(),
        "another owner keeps the shared lock exclusive"
    );
    external_lock.unlock().unwrap();
}

#[test]
fn file_lock_cleanup_preserves_another_live_plugin_call() {
    let dir = tempfile::tempdir().unwrap();
    let first_path = dir.path().join("first.lock");
    let second_path = dir.path().join("second.lock");
    let first = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(first_path)
        .unwrap();
    let second = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&second_path)
        .unwrap();
    first.try_lock().unwrap();
    second.try_lock().unwrap();
    held_file_locks().lock().insert(
        "first-call-lock".into(),
        HeldFileLock {
            call_id: "first-call".into(),
            file: first,
        },
    );
    held_file_locks().lock().insert(
        "second-call-lock".into(),
        HeldFileLock {
            call_id: "second-call".into(),
            file: second,
        },
    );

    release_call_file_locks("first-call");

    let locks = held_file_locks().lock();
    assert!(!locks.contains_key("first-call-lock"));
    assert!(locks.contains_key("second-call-lock"));
    drop(locks);
    let mut probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(second_path)
        .unwrap();
    assert!(
        probe.try_lock().is_err(),
        "the concurrent call keeps its OS lock"
    );
    release_call_file_locks("second-call");
    probe.try_lock().unwrap();
    probe.unlock().unwrap();
}

#[test]
fn read_file_bounded_head_and_tail_windows() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.jsonl");
    let body: String = (0..1000).map(|i| format!("line-{i:04}\n")).collect();
    std::fs::write(&p, &body).unwrap();
    let path = p.to_string_lossy().into_owned();

    let head = read_file_bounded(&path, 32, false).unwrap();
    assert_eq!(head.len(), 32, "head is bounded to max_bytes");
    assert!(body.starts_with(&head), "head is the file prefix");

    let tail = read_file_bounded(&path, 40, true).unwrap();
    assert_eq!(tail.len(), 40, "tail is bounded to max_bytes");
    assert!(body.ends_with(&tail), "tail is the file suffix");

    // Asking for more than the file has returns the whole file, not padding.
    let whole = read_file_bounded(&path, (body.len() as u64) + 4096, true).unwrap();
    assert_eq!(whole, body);
}

#[test]
fn read_file_bounded_trims_partial_utf8_at_cut() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("utf8.txt");
    // Each snowman "☃" is 3 bytes (E2 98 83); pad so cuts land mid-codepoint.
    let body = "☃☃☃☃☃☃☃☃".to_string();
    std::fs::write(&p, &body).unwrap();
    let path = p.to_string_lossy().into_owned();

    // 4 bytes = one full snowman (3) + one dangling lead byte → trailing partial dropped.
    let head = read_file_bounded(&path, 4, false).unwrap();
    assert_eq!(head, "☃", "head drops a partial trailing codepoint");
    assert!(std::str::from_utf8(head.as_bytes()).is_ok());

    // 4 bytes from the end = two dangling continuation bytes + one full snowman.
    let tail = read_file_bounded(&path, 4, true).unwrap();
    assert_eq!(tail, "☃", "tail drops a partial leading codepoint");
    assert!(std::str::from_utf8(tail.as_bytes()).is_ok());
}

#[test]
fn read_file_bounded_never_writes_or_creates() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("session.jsonl");
    let body = "first\nsecond\nthird\n".to_string();
    std::fs::write(&p, &body).unwrap();
    let path = p.to_string_lossy().into_owned();
    let before = std::fs::metadata(&p).unwrap().len();

    let _ = read_file_bounded(&path, 4, false);
    let _ = read_file_bounded(&path, 4, true);

    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        body,
        "content untouched"
    );
    assert_eq!(
        std::fs::metadata(&p).unwrap().len(),
        before,
        "length untouched"
    );

    // A missing path returns None and must not create the file.
    let missing = dir.path().join("does-not-exist.jsonl");
    let mpath = missing.to_string_lossy().into_owned();
    assert!(read_file_bounded(&mpath, 16, false).is_none());
    assert!(read_file_bounded(&mpath, 16, true).is_none());
    assert!(!missing.exists(), "bounded read never creates the target");
}

#[test]
fn host_bounded_reads_exposed_through_preamble() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.jsonl");
    std::fs::write(&p, "META\nMIDDLE\nTAIL").unwrap();
    let path = p.to_string_lossy().into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                head(p, n) { return host.fs.readFileHead(p, n); },
                tail(p, n) { return host.fs.readFileTail(p, n); },
            };
        "#,
    );
    let head: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "head", &[json!(path), json!(4)]).unwrap();
    assert_eq!(head.as_deref(), Some("META"));
    let tail: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "tail", &[json!(path), json!(4)]).unwrap();
    assert_eq!(tail.as_deref(), Some("TAIL"));
}

#[test]
fn host_write_file_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir
        .path()
        .join("a/b/c/hello.txt")
        .to_string_lossy()
        .into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                write(p, c) { return host.writeFile(p, c); }
            };
        "#,
    );
    let ok: bool =
        crate::js_runtime::vm::call_json(&vm, "write", &[json!(nested), json!("nested")]).unwrap();
    assert!(ok);
    assert_eq!(
        std::fs::read_to_string(&nested).unwrap(),
        "nested".to_string()
    );
}

#[test]
fn host_read_dir_json_lists_entries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "bb").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                list(p) {
                    const j = host.readDirJson(p);
                    return JSON.parse(j).map(e => ({name: e.name, isDir: e.isDir}))
                        .sort((x, y) => x.name.localeCompare(y.name));
                }
            };
        "#,
    );
    let entries: Vec<serde_json::Value> =
        crate::js_runtime::vm::call_json(&vm, "list", &[json!(path)]).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["name"], "a.txt");
    assert_eq!(entries[0]["isDir"], false);
    assert_eq!(entries[2]["name"], "sub");
    assert_eq!(entries[2]["isDir"], true);
}

#[test]
fn host_walk_dir_json_depth_bounded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    std::fs::write(dir.path().join("a/x.txt"), "x").unwrap();
    std::fs::write(dir.path().join("a/b/y.txt"), "y").unwrap();
    std::fs::write(dir.path().join("a/b/c/z.txt"), "z").unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                walk(p, d) { return JSON.parse(host.walkDirJson(p, d)).map(e => e.name); }
            };
        "#,
    );
    let names1: Vec<String> =
        crate::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(1)]).unwrap();
    assert!(names1.contains(&"a".into()));
    assert!(names1.contains(&"x.txt".into()));
    assert!(
        !names1.contains(&"z.txt".into()),
        "depth=1 must not reach z.txt"
    );

    let names_deep: Vec<String> =
        crate::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(5)]).unwrap();
    assert!(names_deep.contains(&"z.txt".into()));
}

#[test]
fn host_shell_quote_posix_escapes_single_quote() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                q(s) { return host.shellQuote(s); }
            };
        "#,
    );
    let out: String = crate::js_runtime::vm::call_json(&vm, "q", &[json!("don't")]).unwrap();
    assert_eq!(out, "'don'\\''t'");
}

#[test]
fn host_shell_quote_for_windows_uses_double_single_quote() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                q(s) { return host.shellQuoteFor(s, "windows"); }
            };
        "#,
    );
    let out: String = crate::js_runtime::vm::call_json(&vm, "q", &[json!("can't")]).unwrap();
    assert_eq!(out, "'can''t'");
}

#[test]
fn host_shell_env_prefix_posix_uses_inline_assignment_no_chaining() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                p(n, v) { return host.shellEnvPrefix(n, v, "linux"); }
            };
        "#,
    );
    let out: String = crate::js_runtime::vm::call_json(
        &vm,
        "p",
        &[json!("CLAUDE_CODE_DISABLE_HOOKS"), json!("1")],
    )
    .unwrap();
    assert_eq!(out, "CLAUDE_CODE_DISABLE_HOOKS='1' ");
    assert!(
        !out.contains("&&") && !out.contains(';'),
        "POSIX env prefix must not need a statement joiner: {out:?}"
    );
}

#[test]
fn host_shell_env_prefix_windows_uses_env_drive_and_semicolon() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                p(n, v) { return host.shellEnvPrefix(n, v, "windows"); }
            };
        "#,
    );
    let out: String = crate::js_runtime::vm::call_json(
        &vm,
        "p",
        &[json!("CLAUDE_CODE_DISABLE_HOOKS"), json!("1")],
    )
    .unwrap();
    assert_eq!(out, "$env:CLAUDE_CODE_DISABLE_HOOKS='1'; ");
    assert!(
        !out.contains("&&"),
        "windows env prefix must not use POSIX `&&`: {out:?}"
    );
}

#[test]
fn host_shell_env_prefix_windows_quotes_embedded_single_quote() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                p(n, v) { return host.shellEnvPrefix(n, v, "windows"); }
            };
        "#,
    );
    let out: String =
        crate::js_runtime::vm::call_json(&vm, "p", &[json!("X"), json!("can't")]).unwrap();
    assert_eq!(out, "$env:X='can''t'; ");
}

#[test]
fn host_write_temp_file_returns_real_path() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                mk(c) { return host.writeTempFile(c, ".task.txt"); }
            };
        "#,
    );
    let path: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "mk", &[json!("payload payload")]).unwrap();
    let path = path.unwrap();
    assert!(path.ends_with(".task.txt"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload payload");
    let _ = std::fs::remove_file(&path);
}

/// A CONFINED plugin's `host.writeTempFile` must still succeed, and the
/// result must land under that plugin's own scoped temp subdir — never the
/// bare system temp dir another plugin (or process) can also reach.
#[test]
fn host_write_temp_file_confined_lands_under_the_plugin_scoped_dir() {
    use crate::js_runtime::manifest::PermissionsConfig;
    use crate::js_runtime::vm::PluginVm;

    let plugin_id = format!("confined-temp-writer-{}", uuid::Uuid::new_v4().simple());
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(&plugin_id, &PermissionsConfig::default()),
    );
    let vm = PluginVm::from_source_sandboxed(
        &plugin_id,
        r#"
            module.exports.default = {
                mk(c) { return host.writeTempFile(c, ".task.txt"); }
            };
        "#
        .to_string(),
        None,
        sandbox,
    )
    .expect("vm");

    let path: Option<String> =
        crate::js_runtime::vm::call_json(&vm, "mk", &[json!("payload payload")]).unwrap();
    let path = path.expect("writeTempFile must still succeed when confined");

    let scoped_dir = crate::js_runtime::loader::plugin_temp_dir(&plugin_id);
    assert!(
        std::path::Path::new(&path).starts_with(&scoped_dir),
        "{path} must be under the plugin's own scoped temp dir {}",
        scoped_dir.display()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload payload");
    let _ = std::fs::remove_dir_all(&scoped_dir);
}

#[test]
fn host_unix_now_monotonic_ish() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                now() { return host.unixNow(); },
                nowMs() { return host.unixNowMs(); }
            };
        "#,
    );
    let s: i64 = crate::js_runtime::vm::call_json(&vm, "now", &[]).unwrap();
    let ms: i64 = crate::js_runtime::vm::call_json(&vm, "nowMs", &[]).unwrap();
    assert!(s > 1_700_000_000, "unix seconds should be a real timestamp");
    assert!(ms > s * 1_000 - 5_000 && ms < (s + 5) * 1_000);
}

#[test]
fn host_log_does_not_crash() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                go() { host.log("info", "hello from plugin"); return 1; }
            };
        "#,
    );
    let r: i32 = crate::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
    assert_eq!(r, 1);
}

#[test]
fn host_sqlite_query_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE k(id INTEGER PRIMARY KEY, name TEXT, score REAL);\
                 INSERT INTO k VALUES (1,'alpha',1.5),(2,'beta',2.5),(3,'gamma',3.5);",
        )
        .unwrap();
    }
    let db_path = db.to_string_lossy().into_owned();
    let vm = vm_with(
        r#"
            module.exports.default = {
                topByName(p) {
                    return JSON.parse(host.sqliteQueryAllJson(
                        p, "SELECT id, name, score FROM k WHERE score > ?1 ORDER BY name DESC", "[2.0]"));
                }
            };
        "#,
    );
    let rows: Vec<serde_json::Value> =
        crate::js_runtime::vm::call_json(&vm, "topByName", &[json!(db_path)]).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "gamma");
    assert_eq!(rows[0]["id"], 3);
    assert_eq!(rows[1]["name"], "beta");
}

#[test]
fn host_sqlite_handles_missing_db_gracefully() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                run() { return host.sqliteQueryAllJson("/no/such/db.sqlite", "SELECT 1", null); }
            };
        "#,
    );
    let r: Option<String> = crate::js_runtime::vm::call_json(&vm, "run", &[]).unwrap();
    assert_eq!(r, None);
}

#[test]
fn host_install_idempotent_across_reload() {
    let vm = vm_with(
        r#"
            module.exports.default = {
                go() { return host.platform(); }
            };
        "#,
    );
    let p1: String = crate::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
    vm.reload(
        r#"module.exports.default = { go() { return host.platform() + "!"; } };"#.to_string(),
    )
    .unwrap();
    let p2: String = crate::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
    assert_eq!(p2, format!("{p1}!"));
}

/// A tree wider than the budget is truncated AT the budget rather than walked
/// whole: the depth limit alone bounds nothing, which is how one provider
/// session root pegged a daemon's CPU and starved everything behind it (#319).
#[test]
fn walk_truncates_a_wide_tree_at_the_entry_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let over = WALK_ENTRY_BUDGET + 25;
    for i in 0..over {
        std::fs::write(root.join(format!("f{i}.jsonl")), b"x").unwrap();
    }

    let walked: Vec<serde_json::Value> =
        serde_json::from_str(&walk_dir_json(root.to_str().unwrap(), 8).unwrap()).unwrap();
    assert_eq!(
        walked.len(),
        WALK_ENTRY_BUDGET,
        "a walk must stop at its budget, not at the end of a wide directory"
    );
}

/// Under the budget nothing changes: every entry is still reported, so bounding
/// the walk did not quietly shrink an ordinary result.
#[test]
fn walk_under_the_budget_reports_every_entry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(root.join("a.jsonl"), b"x").unwrap();
    std::fs::write(root.join("nested").join("b.jsonl"), b"x").unwrap();

    let walked: Vec<serde_json::Value> =
        serde_json::from_str(&walk_dir_json(root.to_str().unwrap(), 8).unwrap()).unwrap();
    let names: Vec<&str> = walked
        .iter()
        .filter_map(|entry| entry.get("name").and_then(|n| n.as_str()))
        .collect();
    assert_eq!(walked.len(), 3, "two files and one directory");
    for expected in ["a.jsonl", "nested", "b.jsonl"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
}

/// The confinement heuristic is a substring match, so it cannot tell a host-set
/// directory path from a credential. Both halves live in one test on purpose: an
/// allowlist that leaks, or a heuristic that stops refusing, fails here rather
/// than passing because the other half was checked somewhere else.
#[test]
fn host_set_path_vars_are_readable_while_the_api_token_stays_refused() {
    let sb = PluginSandbox::restricted(vec![], vec![]);

    for name in HOST_PROVIDED_ENV.iter().copied() {
        assert!(
            sb.can_env(name),
            "{name} is host-set and carries a path, not a secret"
        );
    }
    assert!(
        sb.can_env("CODETERM_CODEX_SESSIONS_ROOT"),
        "the codex sessions root is the variable the substring rule wrongly refused"
    );

    assert!(
        !sb.can_env("CODETERM_API_TOKEN"),
        "the allowlist must be exact names — CODETERM_API_TOKEN shares the CODETERM_ prefix"
    );
    for secret in [
        "AWS_SECRET_ACCESS_KEY",
        "ANTHROPIC_API_KEY",
        "GITHUB_PASSWORD",
        "CODEX_SESSION_TOKEN",
    ] {
        assert!(!sb.can_env(secret), "{secret} must stay refused");
    }
}

/// Declaring a credential is what takes direct access to it away: the codex home
/// becomes readable so session detection works, and `auth.json` inside that same
/// home does not, because the host reads it instead.
#[test]
fn a_declared_credential_file_stays_unreadable_inside_its_own_read_root() {
    use crate::js_runtime::manifest::CredentialConfig;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let sessions = home.join("session_index.jsonl");
    std::fs::write(&sessions, "{}").unwrap();
    let auth = home.join("auth.json");
    std::fs::write(&auth, r#"{"tokens":{"access_token":"t"}}"#).unwrap();

    let credential = CredentialConfig {
        id: "codexAuth".into(),
        file: auth.to_string_lossy().into_owned(),
        public: Default::default(),
        secret: Default::default(),
    };
    let sb = PluginSandbox::restricted(vec![home.clone()], vec![])
        .with_credentials(std::slice::from_ref(&credential));

    assert!(
        sb.can_read(sessions.to_str().unwrap()),
        "operational data under the declared root must be readable"
    );
    assert!(
        !sb.can_read(auth.to_str().unwrap()),
        "the declared credential file is the host's to read, never the plugin's"
    );
}

/// Every refusal has to leave a trace a caller can act on, or a provider that
/// lost a capability just returns nothing and the panel goes quiet.
#[test]
fn each_denial_advances_the_counter_and_an_allowed_call_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("root");
    std::fs::create_dir_all(&allowed).unwrap();
    let inside = allowed.join("ok.json");
    std::fs::write(&inside, "{}").unwrap();

    let sb = PluginSandbox::restricted(vec![allowed], vec![]);
    assert_eq!(sb.denial_count(), 0);

    assert!(sb.can_read(inside.to_str().unwrap()));
    assert_eq!(sb.denial_count(), 0, "an allowed read is not a denial");

    assert!(!sb.can_read("/etc/passwd"));
    assert_eq!(sb.denial_count(), 1);

    assert!(!sb.can_env("CODETERM_API_TOKEN"));
    assert_eq!(
        sb.denial_count(),
        2,
        "a refused env var is a lost capability too"
    );

    assert!(sb.can_env("CODETERM_CODEX_SESSIONS_ROOT"));
    assert_eq!(sb.denial_count(), 2, "an allowlisted name is not a denial");
}
