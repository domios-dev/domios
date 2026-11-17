use super::*;
use crate::js_runtime::{manifest::PluginManifest, vm::VmError};
use rquickjs::{function::Func, CatchResultExt, Ctx};

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

    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    super::io::install(&host, &sandbox, &provider_observation)?;
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
        crate::provider_account::environment(&name).and_then(|value| value.into_string().ok())
    });
    bind!("unixNow", || -> i64 { since_epoch().as_secs() as i64 });
    bind!("unixNowMs", || -> i64 { since_epoch().as_millis() as i64 });
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
    bind!("toolcallParse", |raw_text: String,
                            schema_json: String|
     -> String {
        crate::js_runtime::toolcall_parse::parse_to_json(&raw_text, &schema_json)
    });
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
    bind!("log", |level: String, msg: String| {
        match level.as_str() {
            "trace" => log::trace!("[plugin] {msg}"),
            "debug" => log::debug!("[plugin] {msg}"),
            "warn" => log::warn!("[plugin] {msg}"),
            "error" => log::error!("[plugin] {msg}"),
            _ => log::info!("[plugin] {msg}"),
        }
    });
    let sb = sandbox.clone();
    bind!("settingsJson", move || -> String { sb.settings_json() });

    crate::js_runtime::host_agent_run::install(&host, &sandbox)?;
    crate::js_runtime::host_surface::install(&host)?;

    #[cfg(test)]
    crate::js_runtime::call_gate::install(&host, &sandbox)?;

    ctx.globals()
        .set(HOST_GLOBAL, host)
        .catch(ctx)
        .map_err(|e| VmError::Script(format!("set host global: {e:?}")))?;
    Ok(())
}
