use super::*;

fn windows_rebase() -> PathRebase {
    PathRebase::with_platform(
        Path::new(r"\\?\C:\Users\Owner\.codeterm-dev"),
        Path::new(r"\\?\C:\Users\Owner\.domios-dev"),
        true,
    )
    .unwrap()
}

fn unix_rebase() -> PathRebase {
    PathRebase::with_platform(
        Path::new("/home/owner/.codeterm"),
        Path::new("/home/owner/.domios"),
        false,
    )
    .unwrap()
}

#[test]
fn stored_paths_rebase_value_keeps_each_separator_spelling() {
    let rebase = windows_rebase();
    for (stored, expected) in [
        (
            "C:/Users/Owner/.codeterm-dev/general",
            "C:/Users/Owner/.domios-dev/general",
        ),
        (
            r"C:\Users\Owner\.codeterm-dev\general",
            r"C:\Users\Owner\.domios-dev\general",
        ),
        (
            r"C:/Users/Owner/.codeterm-dev\channels\core",
            r"C:/Users/Owner/.domios-dev\channels\core",
        ),
        (
            r"C:\Users\Owner\.codeterm-dev",
            r"C:\Users\Owner\.domios-dev",
        ),
        (
            r"c:\users\owner\.CODETERM-DEV\general",
            r"c:\users\owner\.domios-dev\general",
        ),
        (
            r"\\?\C:\Users\Owner\.codeterm-dev\provisioned-workspaces\a",
            r"\\?\C:\Users\Owner\.domios-dev\provisioned-workspaces\a",
        ),
        (
            "//?/C:/Users/Owner/.codeterm-dev/general",
            "//?/C:/Users/Owner/.domios-dev/general",
        ),
        (
            "file:c:/users/owner/.codeterm-dev/skills/skill.md",
            "file:c:/users/owner/.domios-dev/skills/skill.md",
        ),
    ] {
        assert_eq!(
            rebase.rebase_value(stored).as_deref(),
            Some(expected),
            "{stored}"
        );
    }
}

#[test]
fn stored_paths_rebase_value_leaves_paths_outside_the_legacy_root() {
    let rebase = windows_rebase();
    for stored in [
        r"C:\Users\Owner\.codeterm-dev-other\general",
        r"C:\Users\Owner\.codeterm-devx",
        r"C:\Users\Owner\.domios-dev\general",
        r"C:\Users\Other\.codeterm-dev\general",
        r"D:\Developer\project\.codeterm-dev\plans",
        r"see C:\Users\Owner\.codeterm-dev\general",
        "",
    ] {
        assert_eq!(rebase.rebase_value(stored), None, "{stored}");
    }
}

#[test]
fn stored_paths_unix_rebase_is_case_and_separator_exact() {
    let rebase = unix_rebase();
    assert_eq!(
        rebase
            .rebase_value("/home/owner/.codeterm/general")
            .as_deref(),
        Some("/home/owner/.domios/general")
    );
    for stored in [
        "/home/owner/.CODETERM/general",
        r"/home/owner/.codeterm\general",
        "/home/owner/.codeterm-other/general",
    ] {
        assert_eq!(rebase.rebase_value(stored), None, "{stored}");
    }
}

#[test]
fn stored_paths_rebase_json_rewrites_string_values_only() {
    let rebase = windows_rebase();
    let stored = r#"{"cwd":"C:\\Users\\Owner\\.codeterm-dev\\general","path":"C:/Users/Owner/.codeterm-dev\\channels\\core","escaped":"C:\/Users\/Owner\/.codeterm-dev\/x","plan":"\\\\?\\C:\\Users\\Owner\\.codeterm-dev\\plans","note":"moved to C:\\Users\\Owner\\.codeterm-dev\\general","quoted":"say \"C:\\Users\\Owner\\.codeterm-dev\\general\"","sibling":"C:\\Users\\Owner\\.codeterm-dev-other"}"#;
    let expected = r#"{"cwd":"C:\\Users\\Owner\\.domios-dev\\general","path":"C:/Users/Owner/.domios-dev\\channels\\core","escaped":"C:\/Users\/Owner\/.domios-dev\/x","plan":"\\\\?\\C:\\Users\\Owner\\.domios-dev\\plans","note":"moved to C:\\Users\\Owner\\.codeterm-dev\\general","quoted":"say \"C:\\Users\\Owner\\.codeterm-dev\\general\"","sibling":"C:\\Users\\Owner\\.codeterm-dev-other"}"#;
    assert_eq!(rebase.rebase_json(stored).as_deref(), Some(expected));
    assert_eq!(rebase.rebase_json(expected), None);
    serde_json::from_str::<serde_json::Value>(expected).unwrap();
}

struct Stores {
    _home: tempfile::TempDir,
    legacy: PathBuf,
    data: PathBuf,
    rebase: PathRebase,
}

fn stores() -> Stores {
    let home = tempfile::tempdir().unwrap();
    let legacy = home.path().join(".codeterm-dev");
    let data = home.path().join(".domios-dev");
    std::fs::create_dir_all(data.join("storage")).unwrap();
    std::fs::create_dir_all(data.join("mem")).unwrap();
    let mut rebase = PathRebase::new(&legacy, &data).unwrap();
    rebase.home_root = true;
    Stores {
        _home: home,
        legacy,
        data,
        rebase,
    }
}

fn separators() -> &'static [char] {
    if cfg!(windows) {
        &['/', '\\']
    } else {
        &['/']
    }
}

fn spelled(root: &Path, separator: char) -> String {
    root.display()
        .to_string()
        .replace(['/', '\\'], &separator.to_string())
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap()
}

fn seed(stores: &Stores, root: &str, separator: char, sibling: &str) {
    let under = |relative: &str| format!("{root}{separator}{relative}");
    let state = Connection::open(stores.data.join("storage/state.db")).unwrap();
    state
        .execute_batch(
            "CREATE TABLE workspaces (id TEXT PRIMARY KEY, cwd TEXT, provisioned_cwd TEXT);
             CREATE TABLE desk_layout_nodes (id INTEGER PRIMARY KEY, cwd TEXT, fields_json TEXT);
             CREATE TABLE cron_jobs (id TEXT PRIMARY KEY, cwd TEXT);
             CREATE TABLE blueprints (id TEXT PRIMARY KEY, cwd TEXT NOT NULL);",
        )
        .unwrap();
    let general = under("general");
    state
        .execute(
            "INSERT INTO workspaces VALUES ('general', ?1, ?1), ('sibling', ?2, NULL)",
            [general.as_str(), sibling],
        )
        .unwrap();
    state
        .execute(
            "INSERT INTO desk_layout_nodes VALUES (1, ?1, ?2)",
            [
                general.clone(),
                format!("{{\"cwd\":{}}}", json_string(&general)),
            ],
        )
        .unwrap();
    state
        .execute("INSERT INTO cron_jobs VALUES ('job', ?1)", [&general])
        .unwrap();
    state
        .execute("INSERT INTO blueprints VALUES ('bp', ?1)", [&general])
        .unwrap();

    let plans = Connection::open(stores.data.join("plans.db")).unwrap();
    plans
        .execute_batch(
            "CREATE TABLE plans (id TEXT PRIMARY KEY, project_path TEXT NOT NULL, document_json TEXT NOT NULL);
             CREATE TABLE legacy_imports (root TEXT PRIMARY KEY, imported_at INTEGER NOT NULL);",
        )
        .unwrap();
    plans
        .execute(
            "INSERT INTO plans VALUES ('pl', ?1, ?2)",
            [
                general.clone(),
                format!("{{\"project_path\":{}}}", json_string(&general)),
            ],
        )
        .unwrap();
    plans
        .execute(
            "INSERT INTO legacy_imports VALUES (?1, 1), (?2, 2)",
            [general.clone(), under("plans")],
        )
        .unwrap();
    if let Some(imported) = stores.rebase.rebase_value(&general) {
        plans
            .execute("INSERT INTO legacy_imports VALUES (?1, 3)", [imported])
            .unwrap();
    }

    let closed = Connection::open(stores.data.join("closed_history.db")).unwrap();
    closed
        .execute_batch(
            "CREATE TABLE closed_tab (id INTEGER PRIMARY KEY, cwd TEXT, launch_json TEXT NOT NULL);",
        )
        .unwrap();
    closed
        .execute(
            "INSERT INTO closed_tab VALUES (1, ?1, ?2)",
            [
                general.clone(),
                format!("{{\"args\":[{}]}}", json_string(&general)),
            ],
        )
        .unwrap();

    let memory = Connection::open(stores.data.join("mem/knowledge.db")).unwrap();
    memory
        .execute_batch(
            "CREATE TABLE mem_items (id TEXT PRIMARY KEY, source TEXT, source_path TEXT);",
        )
        .unwrap();
    memory
        .execute(
            "INSERT INTO mem_items VALUES ('soul', ?1, ?2)",
            [format!("file:{}", under("soul.md")), under("soul.md")],
        )
        .unwrap();

    std::fs::write(
        stores.data.join("plugin-channels.json"),
        format!(
            "{{\n  \"channels\": {{\n    \"core\": {{\n      \"path\": {}\n    }}\n  }}\n}}",
            json_string(&under("channels"))
        ),
    )
    .unwrap();
    std::fs::write(
        stores.data.join("mem/workers.json"),
        format!("[{{\"cwd\":{}}}]", json_string(&under("workers"))),
    )
    .unwrap();
}

fn text(stores: &Stores, database: &str, query: &str) -> Vec<String> {
    let connection = Connection::open(stores.data.join(database)).unwrap();
    let mut statement = connection.prepare(query).unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

fn snapshot(stores: &Stores) -> Vec<String> {
    let mut values = Vec::new();
    for (database, query) in [
        (
            "storage/state.db",
            "SELECT coalesce(cwd, '') || '|' || coalesce(provisioned_cwd, '') FROM workspaces ORDER BY id",
        ),
        (
            "storage/state.db",
            "SELECT cwd || '|' || fields_json FROM desk_layout_nodes",
        ),
        ("storage/state.db", "SELECT cwd FROM cron_jobs"),
        ("storage/state.db", "SELECT cwd FROM blueprints"),
        (
            "plans.db",
            "SELECT project_path || '|' || document_json FROM plans",
        ),
        (
            "plans.db",
            "SELECT root || '|' || imported_at FROM legacy_imports ORDER BY root",
        ),
        (
            "closed_history.db",
            "SELECT cwd || '|' || launch_json FROM closed_tab",
        ),
        (
            "mem/knowledge.db",
            "SELECT source || '|' || source_path FROM mem_items",
        ),
    ] {
        values.extend(text(stores, database, query));
    }
    for file in JSON_FILES {
        values.push(std::fs::read_to_string(stores.data.join(file)).unwrap());
    }
    values
}

#[test]
fn stored_paths_rebase_every_store_in_both_spellings() {
    for &separator in separators() {
        let stores = stores();
        let root = spelled(&stores.legacy, separator);
        let current = spelled(&stores.data, separator);
        let sibling = format!("{root}-other{separator}general");
        seed(&stores, &root, separator, &sibling);

        let outcome = rebase_stores(&stores.data, &stores.rebase);

        assert!(outcome.notices.is_empty(), "{:?}", outcome.notices);
        assert_eq!(outcome.rewritten, 16);
        let under = |relative: &str| format!("{current}{separator}{relative}");
        let general = under("general");
        let values = snapshot(&stores);
        assert_eq!(values[0], format!("{general}|{general}"));
        assert_eq!(values[1], format!("{sibling}|"));
        assert_eq!(
            values[2],
            format!("{general}|{{\"cwd\":{}}}", json_string(&general))
        );
        assert_eq!(values[3..5], [general.clone(), general.clone()]);
        assert_eq!(
            values[5],
            format!("{general}|{{\"project_path\":{}}}", json_string(&general))
        );
        assert_eq!(
            values[6..8],
            [format!("{general}|3"), format!("{}|2", under("plans"))]
        );
        assert_eq!(
            values[8],
            format!("{general}|{{\"args\":[{}]}}", json_string(&general))
        );
        assert_eq!(
            values[9],
            format!("file:{}|{}", under("soul.md"), under("soul.md"))
        );
        assert!(values[10].contains(&json_string(&under("channels"))));
        assert_eq!(
            values[11],
            format!("[{{\"cwd\":{}}}]", json_string(&under("workers")))
        );
    }
}

#[test]
fn stored_paths_second_run_is_a_no_op() {
    let stores = stores();
    let legacy_spelled = stores.legacy.display().to_string();
    seed(
        &stores,
        &legacy_spelled,
        std::path::MAIN_SEPARATOR,
        "unrelated",
    );
    assert!(rebase_stores(&stores.data, &stores.rebase).rewritten > 0);
    let first = snapshot(&stores);
    let modified = std::fs::metadata(stores.data.join("plugin-channels.json"))
        .unwrap()
        .modified()
        .unwrap();

    let outcome = rebase_stores(&stores.data, &stores.rebase);

    assert_eq!(outcome.rewritten, 0);
    assert!(outcome.notices.is_empty());
    assert_eq!(snapshot(&stores), first);
    assert_eq!(
        std::fs::metadata(stores.data.join("plugin-channels.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
}

#[test]
fn stored_paths_without_matches_leave_every_store_untouched() {
    let stores = stores();
    let elsewhere = stores
        .data
        .with_file_name("elsewhere")
        .display()
        .to_string();
    seed(&stores, &elsewhere, std::path::MAIN_SEPARATOR, "unrelated");
    let before = snapshot(&stores);

    let outcome = rebase_stores(&stores.data, &stores.rebase);

    assert_eq!(outcome.rewritten, 0);
    assert!(outcome.notices.is_empty());
    assert_eq!(snapshot(&stores), before);
}

#[test]
fn stored_paths_stay_while_the_legacy_root_exists() {
    let stores = stores();
    let legacy_spelled = stores.legacy.display().to_string();
    seed(
        &stores,
        &legacy_spelled,
        std::path::MAIN_SEPARATOR,
        "unrelated",
    );
    std::fs::create_dir_all(&stores.legacy).unwrap();
    let before = snapshot(&stores);

    let outcome = rebase_stores(&stores.data, &stores.rebase);

    assert_eq!(outcome.rewritten, 0);
    assert_eq!(snapshot(&stores), before);
}

#[test]
fn stored_paths_skip_missing_stores_and_columns() {
    let stores = stores();
    let state = Connection::open(stores.data.join("storage/state.db")).unwrap();
    state
        .execute_batch("CREATE TABLE workspaces (id TEXT PRIMARY KEY, name TEXT);")
        .unwrap();
    drop(state);

    let outcome = rebase_stores(&stores.data, &stores.rebase);

    assert_eq!(outcome.rewritten, 0);
    assert!(outcome.notices.is_empty(), "{:?}", outcome.notices);
    assert!(!stores.data.join("plans.db").exists());
    assert!(!stores.data.join("plugin-channels.json").exists());
}

#[test]
fn stored_paths_rebase_crons_and_nested_plugin_config_idempotently() {
    let stores = stores();
    let old = stores.legacy.join("general").display().to_string();
    let new = stores.data.join("general").display().to_string();
    let crons = serde_json::json!({"jobs":[{"cwd":old}, {"cwd":"~/outside"}]});
    let config = serde_json::json!({"plugins":{"fixture":{"dataRoot":"~/.codeterm-dev/fixture", "other":"~/.codeterm-dev-other/fixture"}}});
    std::fs::write(stores.data.join("crons.json"), crons.to_string()).unwrap();
    std::fs::write(stores.data.join("config.json"), config.to_string()).unwrap();
    let first = rebase_stores(&stores.data, &stores.rebase);
    assert_eq!(first.rewritten, 2);
    assert!(first.notices.is_empty());
    let crons: serde_json::Value =
        serde_json::from_slice(&std::fs::read(stores.data.join("crons.json")).unwrap()).unwrap();
    assert_eq!(crons["jobs"][0]["cwd"], new);
    assert_eq!(crons["jobs"][1]["cwd"], "~/outside");
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(stores.data.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        config["plugins"]["fixture"]["dataRoot"],
        "~/.domios-dev/fixture"
    );
    assert_eq!(
        config["plugins"]["fixture"]["other"],
        "~/.codeterm-dev-other/fixture"
    );
    let second = rebase_stores(&stores.data, &stores.rebase);
    assert_eq!(second.rewritten, 0);
    assert!(second.notices.is_empty());
}

#[test]
fn stored_paths_tilde_rewrite_is_scoped_to_the_exact_instance() {
    let mut rebase = PathRebase::with_platform(
        Path::new("C:/Users/Owner/.codeterm-dev-fixture"),
        Path::new("C:/Users/Owner/.domios-dev-fixture"),
        true,
    )
    .unwrap();
    rebase.home_root = true;
    assert_eq!(
        rebase.rebase_value("~/.codeterm-dev-fixture/general"),
        Some("~/.domios-dev-fixture/general".into())
    );
    assert_eq!(rebase.rebase_value("~/.codeterm/general"), None);
    assert_eq!(
        rebase.rebase_value("~/.codeterm-dev-fixture-other/general"),
        None
    );
}

#[test]
fn stored_paths_custom_root_does_not_rewrite_home_shorthand() {
    let rebase = PathRebase::with_platform(
        Path::new("/custom/.codeterm"),
        Path::new("/custom/.domios"),
        false,
    )
    .unwrap();
    assert_eq!(rebase.rebase_value("~/.codeterm/general"), None);
    assert_eq!(
        rebase.rebase_value("/custom/.codeterm/general"),
        Some("/custom/.domios/general".into())
    );
}
