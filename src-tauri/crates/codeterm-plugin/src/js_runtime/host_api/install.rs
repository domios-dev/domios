use super::*;
use crate::js_runtime::{manifest::PluginManifest, vm::VmError};
use rquickjs::{function::Func, CatchResultExt, Ctx};
use std::fs::OpenOptions;

/// Install native host bindings before the plugin bundle evaluates.
pub(in crate::js_runtime) fn install(
    ctx: &Ctx<'_>,
    manifest_json: Option<&str>,
    sandbox: Arc<PluginSandbox>,
) -> Result<(), VmError> {
    let provider_observation = Arc::new(
        crate::js_runtime::provider_observation::ProviderObservation::from_manifest(manifest_json),
    );
    let host = rquickjs::Object::new(ctx.clone())
        .map_err(|e| VmError::Script(format!("host obj: {e}")))?;

    // Tiny helper to fold the repeated `host.set(name, Func::new(closure)) .map_err(|e| VmError::Script(format!("set {name}: {e}")))?` shape.
    // Reads top-to-bottom as a flat table of (name, closure).
    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    let manifest_owned: Option<String> = manifest_json.map(|s| s.to_string());
    bind!(
        "commandTranscriptStdout",
        move |record_json: String, declaration_json: String| -> Option<String> {
            if record_json.len() > PLUGIN_READ_FILE_LIMIT as usize
                || declaration_json.len() > 24 * 1024
            {
                return None;
            }
            let source: crate::provider::ModelSwitchTranscript =
                serde_json::from_str(&declaration_json).ok()?;
            let record = serde_json::from_str(&record_json).ok()?;
            source.record_stdout(&record)
        }
    );
    bind!("manifestJson", move || -> Option<String> {
        manifest_owned.clone()
    });
    let sb = sandbox.clone();
    let observation = provider_observation.clone();
    bind!("providerVersionJson", move || -> String {
        serde_json::to_string(&observation.version(&sb)).unwrap_or_else(|_| "null".into())
    });

    bind!("platform", || -> String {
        std::env::consts::OS.to_string()
    });
    bind!("homeDir", || -> Option<String> {
        dirs::home_dir().map(|p| p.to_string_lossy().into_owned())
    });
    bind!("dataDir", || -> String {
        codeterm_paths::data_dir_path()
            .to_string_lossy()
            .into_owned()
    });
    bind!("expandHome", |path: String| -> Option<String> {
        PluginManifest::expand_home(&path).map(|p| p.to_string_lossy().into_owned())
    });
    let sb = sandbox.clone();
    bind!("envGet", move |name: String| -> Option<String> {
        if !sb.can_env(&name) {
            log::warn!("[plugin] denied envGet({name}) for confined plugin");
            return None;
        }
        std::env::var(&name).ok()
    });
    bind!("unixNow", || -> i64 { since_epoch().as_secs() as i64 });
    bind!("unixNowMs", || -> i64 { since_epoch().as_millis() as i64 });

    let sb = sandbox.clone();
    bind!("fileExists", move |path: String| -> bool {
        sb.can_read(&path) && Path::new(&path).exists()
    });
    let sb = sandbox.clone();
    bind!("isSymlink", move |path: String| -> Option<bool> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied isSymlink outside sandbox: {path}");
            return None;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => Some(metadata.file_type().is_symlink()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    });
    let sb = sandbox.clone();
    bind!("readFile", move |ctx: Ctx,
                            path: String|
          -> rquickjs::Result<Option<String>> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFile outside sandbox: {path}");
            return Ok(None);
        }
        read_plugin_file(&path).map_err(|error| rquickjs::Exception::throw_range(&ctx, &error))
    });
    let sb = sandbox.clone();
    bind!("readFileHead", move |path: String,
                                max_bytes: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFileHead outside sandbox: {path}");
            return None;
        }
        read_file_bounded(&path, max_bytes.max(0) as u64, false)
    });
    let sb = sandbox.clone();
    bind!("readFileTail", move |path: String,
                                max_bytes: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFileTail outside sandbox: {path}");
            return None;
        }
        read_file_bounded(&path, max_bytes.max(0) as u64, true)
    });
    let sb = sandbox.clone();
    bind!("writeFile", move |path: String, contents: String| -> bool {
        if !sb.can_write(&path) {
            log::warn!("[plugin] denied writeFile outside sandbox: {path}");
            return false;
        }
        let p = Path::new(&path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        std::fs::write(p, contents).is_ok()
    });
    let sb = sandbox.clone();
    bind!("writeFileAtomic", move |path: String,
                                   contents: String|
          -> bool {
        if !sb.can_write(&path) {
            log::warn!("[plugin] denied writeFileAtomic outside sandbox: {path}");
            return false;
        }
        codeterm_util::atomic_write::write_atomic(Path::new(&path), contents).is_ok()
    });
    let sb = sandbox.clone();
    bind!("canonicalPath", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        std::fs::canonicalize(&path)
            .ok()
            .map(|path| codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy()))
    });
    let sb = sandbox.clone();
    bind!("projectWriteAllowed", move |root: String| -> bool {
        sb.project_write_allowed(&root)
    });
    let sb = sandbox.clone();
    bind!("jsonBooleanRecord", move |path: String,
                                     keys_json: String,
                                     grant: bool|
          -> String {
        use crate::provider::PreTrustOutcome;
        let outcome = if !sb.can_read(&path) || (grant && !sb.can_write(&path)) {
            PreTrustOutcome::Skipped("record access denied".into())
        } else if let Ok(keys) = serde_json::from_str::<Vec<String>>(&keys_json) {
            if grant {
                crate::json_boolean_record::grant(Path::new(&path), &keys)
            } else {
                match crate::json_boolean_record::trusted(Path::new(&path), &keys) {
                    Ok(true) => PreTrustOutcome::AlreadyTrusted,
                    Ok(false) => PreTrustOutcome::Skipped("not trusted".into()),
                    Err(reason) => PreTrustOutcome::Skipped(reason),
                }
            }
        } else {
            PreTrustOutcome::Skipped("invalid key path".into())
        };
        serde_json::to_string(&outcome)
            .unwrap_or_else(|_| r#"{"status":"failed","reason":"encoding failed"}"#.into())
    });
    let sb = sandbox.clone();
    bind!("lockFile", move |path: String| -> Option<String> {
        let call_id = current_file_lock_call()?;
        if !sb.can_write(&path) {
            log::warn!("[plugin] denied lockFile outside sandbox: {path}");
            return None;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .ok()?;
        file.try_lock().ok()?;
        let id = uuid::Uuid::new_v4().to_string();
        held_file_locks().lock().insert(
            id.clone(),
            HeldFileLock {
                call_id,
                path: std::fs::canonicalize(&path).ok()?,
                file,
            },
        );
        Some(id)
    });
    bind!("unlockFile", move |id: String| -> bool {
        if !codeterm_decision::physical::effects_allowed() {
            return false;
        }
        let Some(call_id) = current_file_lock_call() else {
            return false;
        };
        let mut locks = held_file_locks().lock();
        if !locks.get(&id).is_some_and(|lock| lock.call_id == call_id) {
            return false;
        }
        let Some(file) = locks.remove(&id) else {
            return false;
        };
        drop(locks);
        file.file.unlock().is_ok()
    });
    let sb = sandbox.clone();
    bind!("removeFile", move |path: String| -> bool {
        sb.can_write(&path) && std::fs::remove_file(&path).is_ok()
    });
    let sb = sandbox.clone();
    bind!("makeDirs", move |path: String| -> bool {
        sb.can_write(&path) && std::fs::create_dir_all(&path).is_ok()
    });
    let sb = sandbox.clone();
    bind!("readDirJson", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        read_dir_json(&path)
    });
    let sb = sandbox.clone();
    bind!("listChildDirsJson", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied listChildDirs outside sandbox: {path}");
            return None;
        }
        list_child_dirs_json(&path)
    });
    let sb = sandbox.clone();
    bind!("walkDirJson", move |path: String,
                               max_depth: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        walk_dir_json(&path, max_depth.max(0) as usize)
    });

    bind!("shellQuote", |s: String| -> String {
        posix_single_quote(&s)
    });
    bind!("shellQuoteFor", |s: String, platform: String| -> String {
        match platform.as_str() {
            "windows" => powershell_quote(&s),
            _ => posix_single_quote(&s),
        }
    });
    bind!("shellEnvPrefix", |name: String,
                             value: String,
                             platform: String|
     -> String {
        shell_env_prefix(&name, &value, &platform)
    });
    bind!("writeTempFile", move |contents: String,
                                 suffix: Option<String>|
          -> Option<String> {
        if !codeterm_decision::physical::effects_allowed() {
            return None;
        }
        codeterm_util::task_files::write(&contents, suffix.as_deref())
    });
    bind!("toolcallParse", |raw_text: String,
                            schema_json: String|
     -> String {
        crate::js_runtime::toolcall_parse::parse_to_json(&raw_text, &schema_json)
    });

    // Crypto helpers.
    bind!("md5", |s: String| -> String {
        use md5::{Digest, Md5};
        let mut h = Md5::new();
        h.update(s.as_bytes());
        let digest = h.finalize();
        let mut out = String::with_capacity(32);
        for b in digest {
            out.push_str(&format!("{b:02x}"));
        }
        out
    });

    let sb = sandbox.clone();
    let observation = provider_observation.clone();
    bind!(
        "sqliteQueryAllJson",
        move |ctx: Ctx, db: String, sql: String, params_json: Option<String>| {
            sqlite::host_query(&ctx, &sb, &observation, &db, &sql, params_json.as_deref())
        }
    );
    let sb = sandbox.clone();
    let observation = provider_observation.clone();
    bind!("sqliteQueryStart", move |db: String,
                                    sql: String,
                                    params_json: Option<String>|
          -> String {
        sqlite::host_query_start(&sb, &observation, db, sql, params_json)
    });

    bind!("log", |level: String, msg: String| {
        match level.as_str() {
            "trace" => log::trace!("[plugin] {msg}"),
            "debug" => log::debug!("[plugin] {msg}"),
            "warn" => log::warn!("[plugin] {msg}"),
            "error" => log::error!("[plugin] {msg}"),
            _ => log::info!("[plugin] {msg}"),
        }
    });

    // Each is gated by the manifest permission baked into the sandbox.
    let sb = sandbox.clone();
    bind!("settingsJson", move || -> String { sb.settings_json() });

    let sb = sandbox.clone();
    bind!("exec", move |opts_json: String| -> String {
        let bin = crate::js_runtime::host_exec::bin_of(&opts_json).unwrap_or_default();
        if !sb.can_exec(&bin) {
            log::warn!(
                "[plugin:{}] denied exec({bin}) — not in subprocess.allow",
                sb.plugin_id()
            );
            return serde_json::json!({ "error": format!("exec denied: {bin}") }).to_string();
        }
        crate::js_runtime::host_exec::run_with_host_env(
            &opts_json,
            &crate::js_runtime::host_exec::current_host_env(),
            &sb,
        )
    });

    // Async exec (job-id + poll): same gate as `exec`, but the command runs off-thread so the VM lock is held only per start/poll.
    let sb = sandbox.clone();
    bind!("execStart", move |opts_json: String| -> String {
        let bin = crate::js_runtime::host_exec::bin_of(&opts_json).unwrap_or_default();
        if !sb.can_exec(&bin) {
            log::warn!(
                "[plugin:{}] denied exec.start({bin}) — not in subprocess.allow",
                sb.plugin_id()
            );
            return serde_json::json!({ "error": format!("exec denied: {bin}") }).to_string();
        }
        crate::js_runtime::host_exec::start_with_host_env(
            sb.plugin_id(),
            &opts_json,
            &crate::js_runtime::host_exec::current_host_env(),
            sb.clone(),
        )
    });
    let sb = sandbox.clone();
    bind!("execPoll", move |job_id: String| -> String {
        crate::js_runtime::host_exec::poll(sb.plugin_id(), &job_id)
    });
    let sb = sandbox.clone();
    bind!("execClose", move |job_id: String| {
        if !codeterm_decision::physical::effects_allowed() {
            return;
        }
        crate::js_runtime::host_exec::close(sb.plugin_id(), &job_id);
    });

    let sb = sandbox.clone();
    bind!("credentialPublic", move |id: String| -> Option<String> {
        crate::js_runtime::host_credentials::public_json(sb.credentials(), &id)
    });
    let sb = sandbox.clone();
    bind!("fetch", move |opts_json: String| -> String {
        let host = crate::js_runtime::host_net::host_of(&opts_json).unwrap_or_default();
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        match prepare_fetch(&sb, &opts_json) {
            Ok(prepared) => crate::js_runtime::host_net::run(&prepared),
            Err(e) => serde_json::json!({ "error": e }).to_string(),
        }
    });
    let sb = sandbox.clone();
    bind!("fetchStart", move |opts_json: String| -> String {
        let host = crate::js_runtime::host_net::host_of(&opts_json).unwrap_or_default();
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch.async({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        match prepare_fetch(&sb, &opts_json) {
            Ok(prepared) => crate::js_runtime::host_net::start(sb.plugin_id(), &prepared),
            Err(e) => serde_json::json!({ "error": e }).to_string(),
        }
    });
    let sb = sandbox.clone();
    bind!("fetchStream", move |opts_json: String| -> String {
        let host = crate::js_runtime::host_net::host_of(&opts_json).unwrap_or_default();
        // Egress-equivalent to host.fetch: use the exact same network.allow gate.
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch.stream({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        match prepare_fetch(&sb, &opts_json) {
            Ok(prepared) => crate::js_runtime::host_net::stream(sb.plugin_id(), &prepared),
            Err(e) => serde_json::json!({ "error": e }).to_string(),
        }
    });
    let sb = sandbox.clone();
    bind!("fetchStreamPoll", move |job_id: String| -> String {
        if !codeterm_decision::physical::effects_allowed() {
            return serde_json::json!({"error":"effect stream unavailable during observation"})
                .to_string();
        }
        crate::js_runtime::host_net::stream_poll(sb.plugin_id(), &job_id)
    });
    let sb = sandbox.clone();
    bind!("fetchStreamClose", move |job_id: String| {
        if !codeterm_decision::physical::effects_allowed() {
            return;
        }
        crate::js_runtime::host_net::stream_close(sb.plugin_id(), &job_id);
    });

    let sb = sandbox.clone();
    bind!("secretGet", move |name: String| -> Option<String> {
        if !sb.secrets_allowed() {
            return None;
        }
        sb.service()?.secret_get(sb.plugin_id(), &name)
    });
    let sb = sandbox.clone();
    bind!("secretSet", move |name: String, value: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        sb.service()
            .map(|svc| svc.secret_set(sb.plugin_id(), &name, &value))
            .unwrap_or(false)
    });
    let sb = sandbox.clone();
    bind!("secretDelete", move |name: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        sb.service()
            .map(|svc| svc.secret_delete(sb.plugin_id(), &name))
            .unwrap_or(false)
    });

    // host compute base (host.worker/agent/workspace/mem) — lives in `host_compute` to keep this bind table flat.
    crate::js_runtime::host_transfer::install_handles(&host, &sandbox)?;
    crate::js_runtime::host_compute::install_handles(&host, &sandbox)?;

    #[cfg(test)]
    crate::js_runtime::call_gate::install(&host, &sandbox)?;

    ctx.globals()
        .set(HOST_GLOBAL, host)
        .catch(ctx)
        .map_err(|e| VmError::Script(format!("set host global: {e:?}")))?;
    Ok(())
}
