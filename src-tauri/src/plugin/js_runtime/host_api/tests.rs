use super::*;
use crate::plugin::js_runtime::vm::PluginVm;
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

    // Inside the allowed root → ok.
    let inside = allowed.join("a.jsonl");
    std::fs::write(&inside, "{}").unwrap();
    assert!(sb.can_read(inside.to_str().unwrap()));
    assert!(sb.can_write(allowed.join("b.jsonl").to_str().unwrap()));

    // Outside → denied (incl. traversal attempts).
    assert!(!sb.can_read("/etc/passwd"));
    assert!(!sb.can_read(allowed.join("../../../../etc/passwd").to_str().unwrap()));
    assert!(!sb.can_write("/tmp/evil"));

    // envGet: path-ish vars allowed, secret-named blocked.
    assert!(sb.can_env("XDG_DATA_HOME"));
    assert!(sb.can_env("HOME"));
    assert!(!sb.can_env("CODETERM_API_TOKEN"));
    assert!(!sb.can_env("AWS_SECRET_ACCESS_KEY"));
    assert!(!sb.can_env("ANTHROPIC_API_KEY"));
    assert!(!sb.can_env("GITHUB_PASSWORD"));
}

#[test]
fn host_api_v2_gating_by_allowlist() {
    use crate::plugin::js_runtime::manifest::{
        NetworkPermission, PermissionsConfig, SubprocessPermission,
    };
    // Restricted plugin with a narrow grant.
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
    // exec: only the allowlisted binary.
    assert!(sb.can_exec("bw"));
    assert!(!sb.can_exec("rm"));
    // fetch: host or host:port both match the rule.
    assert!(sb.can_reach("127.0.0.1:7891"));
    assert!(sb.can_reach("127.0.0.1"));
    assert!(!sb.can_reach("evil.example.com"));
    assert!(sb.secrets_allowed());
    assert!(sb.agent_commands_allowed());
    assert!(sb.manage_workspaces_allowed());
    assert!(sb.spawn_agents_allowed());

    // Restricted plugin with NO grants → everything denied.
    let none = PluginSandbox::restricted(vec![], vec![])
        .with_identity_and_perms("p2", &PermissionsConfig::default());
    assert!(!none.can_exec("bw"));
    assert!(!none.can_reach("127.0.0.1:7891"));
    assert!(!none.secrets_allowed());
    assert!(!none.agent_commands_allowed());
    assert!(!none.manage_workspaces_allowed());
    assert!(!none.spawn_agents_allowed());

    // Unrestricted (bundled/dev) → everything allowed regardless of lists.
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
fn late_ref_resolves_only_after_population_while_target_lives() {
    // `LateRef` is the carrier `PluginSandbox.svc` uses. `ServiceContext`
    // cannot be built in a unit test (needs a `tauri::AppHandle` + ~40
    // fields), so resolution semantics are proven here against a stand-in
    // target; `PluginSandbox` delegates verbatim to this type.
    struct FakeService;
    let r: LateRef<FakeService> = LateRef::new();
    // Before population → None.
    assert!(r.get().is_none(), "unpopulated LateRef must not resolve");

    let svc = Arc::new(FakeService);
    assert!(r.set(Arc::downgrade(&svc)), "first set must succeed");
    // After population, target alive → Some, and it is the same Arc.
    let resolved = r.get().expect("populated LateRef must resolve");
    assert!(Arc::ptr_eq(&resolved, &svc));

    // First-write-wins: a second set is rejected and does not swap the ref.
    let other = Arc::new(FakeService);
    assert!(
        !r.set(Arc::downgrade(&other)),
        "second set must be rejected"
    );
    assert!(Arc::ptr_eq(&r.get().unwrap(), &svc));

    // Drop every strong owner → the weak ref no longer upgrades.
    drop(resolved);
    drop(svc);
    assert!(
        r.get().is_none(),
        "LateRef must not resolve a dropped target"
    );
}

#[test]
fn sandbox_service_ref_is_late_bound() {
    // A fresh sandbox has no service ref. `set_service` with a live weak
    // makes `service()` resolve; this exercises the real `PluginSandbox`
    // carrier wiring against the real `ServiceContext` type.
    let sb = PluginSandbox::unrestricted();
    assert!(sb.service().is_none(), "sandbox starts with no service ref");
    // A dangling weak (no backing Arc) populates the slot but never resolves.
    assert!(sb.set_service(Weak::<ServiceContext>::new()));
    assert!(sb.service().is_none(), "dead weak ref must resolve to None");
    // Carrier is write-once.
    assert!(!sb.set_service(Weak::<ServiceContext>::new()));
}

#[test]
fn host_settings_json_carries_plugin_config() {
    let sb = PluginSandbox::unrestricted().with_settings(r#"{"engine":"local"}"#.to_string());
    assert_eq!(sb.settings_json(), r#"{"engine":"local"}"#);
    // default is the JSON null literal so JSON.parse() in the plugin succeeds.
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
    let p: String = crate::plugin::js_runtime::vm::call_json(&vm, "getPlatform", &[]).unwrap();
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
    let p: Option<String> = crate::plugin::js_runtime::vm::call_json(&vm, "getHome", &[]).unwrap();
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
        crate::plugin::js_runtime::vm::call_json(&vm, "expand", &[json!("~/foo/bar")]).unwrap();
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
    let ok: bool = crate::plugin::js_runtime::vm::call_json(
        &vm,
        "write",
        &[json!(path), json!("plugin wrote this")],
    )
    .unwrap();
    assert!(ok);
    let exists: bool =
        crate::plugin::js_runtime::vm::call_json(&vm, "exists", &[json!(path)]).unwrap();
    assert!(exists);
    let body: Option<String> =
        crate::plugin::js_runtime::vm::call_json(&vm, "read", &[json!(path)]).unwrap();
    assert_eq!(body.as_deref(), Some("plugin wrote this"));
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
        crate::plugin::js_runtime::vm::call_json(&vm, "write", &[json!(nested), json!("nested")])
            .unwrap();
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
        crate::plugin::js_runtime::vm::call_json(&vm, "list", &[json!(path)]).unwrap();
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
        crate::plugin::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(1)]).unwrap();
    // depth=1 should see "a", and immediate children ("b","x.txt").
    assert!(names1.contains(&"a".into()));
    assert!(names1.contains(&"x.txt".into()));
    assert!(
        !names1.contains(&"z.txt".into()),
        "depth=1 must not reach z.txt"
    );

    let names_deep: Vec<String> =
        crate::plugin::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(5)]).unwrap();
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
    let out: String =
        crate::plugin::js_runtime::vm::call_json(&vm, "q", &[json!("don't")]).unwrap();
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
    let out: String =
        crate::plugin::js_runtime::vm::call_json(&vm, "q", &[json!("can't")]).unwrap();
    assert_eq!(out, "'can''t'");
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
        crate::plugin::js_runtime::vm::call_json(&vm, "mk", &[json!("payload payload")]).unwrap();
    let path = path.unwrap();
    assert!(path.ends_with(".task.txt"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload payload");
    let _ = std::fs::remove_file(&path);
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
    let s: i64 = crate::plugin::js_runtime::vm::call_json(&vm, "now", &[]).unwrap();
    let ms: i64 = crate::plugin::js_runtime::vm::call_json(&vm, "nowMs", &[]).unwrap();
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
    let r: i32 = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
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
        crate::plugin::js_runtime::vm::call_json(&vm, "topByName", &[json!(db_path)]).unwrap();
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
    let r: Option<String> = crate::plugin::js_runtime::vm::call_json(&vm, "run", &[]).unwrap();
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
    let p1: String = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
    vm.reload(
        r#"module.exports.default = { go() { return host.platform() + "!"; } };"#.to_string(),
    )
    .unwrap();
    let p2: String = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
    assert_eq!(p2, format!("{p1}!"));
}
