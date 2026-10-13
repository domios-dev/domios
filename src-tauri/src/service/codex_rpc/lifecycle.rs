pub use super::delivery::DeliveryState;
use super::{
    client::{Client, ClientError},
    process::Process,
};
use crate::service::ServiceContext;
use parking_lot::Mutex;
use rand::RngCore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

pub fn delivery_state(tab: &str) -> DeliveryState {
    super::delivery::state(tab)
}

pub fn startup_attachment_ready(tab: &str) -> bool {
    for_tab(tab).is_some_and(|sidecar| {
        sidecar.verified.load(std::sync::atomic::Ordering::Acquire)
            && super::delivery::attached_for_launch(tab, &sidecar.launch_nonce)
    })
}

pub fn startup_complete(svc: &ServiceContext, tab: &str, nonce: &str) {
    super::delivery::startup_complete(svc, tab, nonce);
}

pub fn plain_launch(svc: &ServiceContext, tab: &str, nonce: &str) {
    super::delivery::plain_launch(svc, tab, nonce);
}

pub fn wait_attached(tab: &str, nonce: &str, timeout: std::time::Duration) -> bool {
    super::delivery::wait_attached(tab, nonce, timeout)
}

pub struct Sidecar {
    pub client: Client,
    pub normalizer: Mutex<super::normalize::Normalizer>,
    pub controls: Mutex<super::model::TurnControls>,
    pub control: Mutex<super::control::ControlState>,
    pub compaction: Mutex<super::compaction::CompactionState>,
    pub turn_settled: tokio::sync::watch::Sender<u64>,
    pub port: u16,
    pub upstream_port: u16,
    pub binary: std::path::PathBuf,
    pub version: String,
    pub thread: Mutex<String>,
    pub verified: std::sync::atomic::AtomicBool,
    pub busy: std::sync::atomic::AtomicBool,
    pub auth: Mutex<super::auth::AuthState>,
    token: String,
    pub thread_settings: Value,
    pub process: Process,
    pub launch_nonce: String,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.client.close("Sidecar released");
    }
}

static STARTING: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static TOKENS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn token_for_tab(tab: &str) -> String {
    TOKENS
        .lock()
        .entry(tab.into())
        .or_insert_with(|| {
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .clone()
}

pub fn launch_env(svc: &ServiceContext, tab: &str, env: &mut HashMap<String, String>) {
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone());
    if let Some(provider) = provider {
        launch_env_for_provider(svc, tab, &provider, env);
    }
}

pub(crate) fn launch_env_for_provider(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    env: &mut HashMap<String, String>,
) {
    if super::launch::requested(svc, provider) {
        env.insert("CODETERM_CODEX_RPC_TOKEN".into(), token_for_tab(tab));
        if for_tab(tab).is_none() {
            super::delivery::begin(tab, &svc.ensure_session_binding_nonce(tab));
        }
    }
}

pub fn for_tab(tab: &str) -> Option<Arc<Sidecar>> {
    crate::service::structured_endpoint::rpc(tab)
}
pub fn stop_tab(tab: &str) {
    TOKENS.lock().remove(tab);
    STARTING.lock().remove(tab);
    super::recovery::completed(tab);
    let sidecar = for_tab(tab);
    if let Some(sidecar) = sidecar {
        log::info!(
            "[codex-rpc-lifecycle] tab={tab} nonce={} action=stop_sidecar reason=tab_lifecycle",
            sidecar.launch_nonce
        );
        super::delivery::disconnected(tab);
        super::approvals::clear_tab(tab);
        crate::service::structured_endpoint::remove(tab);
        sidecar.client.close("Tab lifecycle ended");
        sidecar.process.stop();
    }
}

pub async fn reconnect(tab: &str, sidecar: &Arc<Sidecar>) -> Result<(), String> {
    sidecar
        .verified
        .store(false, std::sync::atomic::Ordering::Release);
    if *sidecar.process.death.borrow() {
        return Err("App-server exited".into());
    }
    sidecar
        .client
        .reconnect(sidecar.upstream_port, &sidecar.token)
        .await?;
    sidecar.client.initialize().await.map_err(rpc_error)?;
    let thread = sidecar.thread.lock().clone();
    let mut settings = sidecar.thread_settings.clone();
    settings["threadId"] = json!(thread);
    settings["excludeTurns"] = json!(true);
    sidecar
        .client
        .request("thread/resume", settings)
        .await
        .map_err(rpc_error)?;
    let read = sidecar
        .client
        .request(
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        )
        .await
        .map_err(rpc_error)?;
    if !super::routing::verified_user_thread(&thread, &read)
        || !for_tab(tab).is_some_and(|current| Arc::ptr_eq(&current, sidecar))
    {
        return Err("Reconnected thread identity unverified".into());
    }
    sidecar.client.release_thread(&thread);
    sidecar
        .verified
        .store(true, std::sync::atomic::Ordering::Release);
    Ok(())
}

#[derive(Clone)]
pub struct Spawn {
    pub provider: String,
    pub cwd: String,
    pub instructions: Option<String>,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub full_access: bool,
}

fn reuse_error(
    live: bool,
    requested_thread: Option<&str>,
    bound_thread: &str,
) -> Option<&'static str> {
    if !live {
        return Some("Existing sidecar exited; explicit restart required");
    }
    if requested_thread.is_some_and(|thread| thread != bound_thread) {
        return Some("Existing sidecar owns another thread; explicit restart required");
    }
    None
}

fn reusable_sidecar(sidecar: Arc<Sidecar>, spawn: &Spawn) -> Result<Arc<Sidecar>, String> {
    let error = reuse_error(
        !*sidecar.process.death.borrow(),
        spawn.resume.as_deref(),
        &sidecar.thread.lock(),
    );
    if let Some(error) = error {
        return Err(error.into());
    }
    Ok(sidecar)
}

#[cfg(test)]
mod reuse_tests {
    use super::reuse_error;

    #[test]
    fn codex_rpc_control_concurrent_recovery_reuses_live_server_without_replacement() {
        assert_eq!(reuse_error(true, None, "own"), None);
        assert_eq!(reuse_error(true, Some("own"), "own"), None);
        assert!(reuse_error(true, Some("other"), "own").is_some());
        assert!(reuse_error(false, Some("own"), "own").is_some());
    }
}

pub async fn ensure_for_spawn(
    svc: &ServiceContext,
    tab: &str,
    spawn: Spawn,
) -> Result<Arc<Sidecar>, String> {
    if let Some(existing) = for_tab(tab) {
        return reusable_sidecar(existing, &spawn);
    }
    let launch_nonce = svc.ensure_session_binding_nonce(tab);
    super::delivery::begin(tab, &launch_nonce);
    let gate = STARTING
        .lock()
        .entry(tab.into())
        .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let _permit = gate.acquire().await.map_err(|error| error.to_string())?;
    if let Some(existing) = for_tab(tab) {
        return reusable_sidecar(existing, &spawn);
    }
    super::startup_retry::run(tab, || start(svc, tab, spawn.clone(), &launch_nonce)).await
}

async fn start(
    svc: &ServiceContext,
    tab: &str,
    spawn: Spawn,
    expected_nonce: &str,
) -> Result<Arc<Sidecar>, String> {
    {
        let mut mux = svc.mux.write();
        let current = mux.get_tab_mut(tab);
        super::startup_retry::verify_launch(
            expected_nonce,
            current
                .as_ref()
                .and_then(|tab| tab.session_binding_nonce.as_deref()),
        )?;
        if let Some(tab) = current {
            tab.structured_control_active = false;
        }
    }
    let (cap, binary) = svc
        .agent_registry
        .with_provider(&spawn.provider, |provider| {
            (
                provider.structured_control(),
                provider.binary_name().to_string(),
            )
        })
        .ok_or("Provider unavailable")?;
    let cap = cap.ok_or("Structured control unavailable")?;
    // Probe, not the listing cache: a restore can launch before any provider listing filled it.
    let codeterm_plugin::js_runtime::BinResolution::Found(binary) =
        codeterm_plugin::js_runtime::probe_bin(&binary).resolution
    else {
        return Err("Provider binary unavailable".into());
    };
    if !binary.is_absolute() {
        return Err(format!("Sidecar binary must be absolute: {binary:?}"));
    }
    if matches!(
        binary.extension().and_then(|ext| ext.to_str()),
        Some("cmd" | "bat")
    ) {
        return Err(format!("Sidecar requires a native executable: {binary:?}"));
    }
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    let root = crate::paths::data_dir().join("codex-rpc");
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("Sidecar runtime directory {root:?}: {error}"))?;
    let generation = uuid::Uuid::new_v4().to_string();
    let token_file = root.join(format!("{generation}.token"));
    let marker_file = root.join(format!("{generation}.json"));
    let token = token_for_tab(tab);
    write_token(&token_file, &token)?;
    let mut artifacts = Artifacts {
        token: token_file.clone(),
        marker: marker_file.clone(),
        committed: false,
    };
    let mut env = svc.tab_env(tab);
    let launch_nonce = env.get("CODETERM_SESSION_BINDING_NONCE").cloned();
    super::startup_retry::verify_launch(expected_nonce, launch_nonce.as_deref())?;
    let attach_nonce = launch_nonce
        .clone()
        .ok_or("Structured launch requires a binding nonce")?;
    env.extend(
        svc.model_provider_launch_env(&spawn.provider)
            .map_err(|error| error.to_string())?,
    );
    let process = super::process::spawn(
        &binary,
        &spawn.cwd,
        env,
        tab,
        port,
        &token_file,
        &marker_file,
    )
    .await?;
    let cleanup = process.cleanup();
    let result = async {
        let client = Client::connect(port, &token).await?;
        let mut client_guard = StartupClient { client: client.clone(), committed: false };
        let init = client.initialize().await.map_err(rpc_error)?;
        let init: super::schema::InitializeResponse =
            serde_json::from_value(init).map_err(|error| error.to_string())?;
        if !super::capability::version_supported(&init.user_agent, &cap.min_version) {
            client.close("Unsupported protocol version");
            return Err("Unsupported app-server protocol version".into());
        }
        let runtime_mode = svc
            .mux
            .read()
            .get_tab(tab)
            .and_then(|tab| tab.restorable_launch.structured_runtime_mode.clone())
            .and_then(|mode| serde_json::from_value::<crate::service::agent_runtime::model::RuntimeMode>(json!(mode)).ok())
            .unwrap_or(if spawn.full_access {
                crate::service::agent_runtime::model::RuntimeMode::FullAccess
            } else {
                crate::service::agent_runtime::model::RuntimeMode::AutoAcceptEdits
            });
        let mut params = super::thread_start::parameters(&spawn, runtime_mode);
        let resume = spawn
            .resume
            .filter(|_| crate::config::provider_config_from(&svc.configuration(), &spawn.provider).resume_sessions.unwrap_or(true));
        let response = if let Some(thread) = resume.as_deref() {
            params["threadId"] = json!(thread);
            params["excludeTurns"] = json!(true);
            resume_thread(&client, params.clone()).await?
        } else {
            client
                .request("thread/start", params.clone())
                .await
                .map_err(rpc_error)?
        };
        let (thread, auth, history) =
            super::thread_start::initial_state(&client, response, resume.is_some()).await?;
        if !svc
            .mux
            .read()
            .get_tab(tab)
            .is_some_and(|tab| tab.session_binding_nonce == launch_nonce)
        {
            client.close("Tab launch superseded");
            return Err("Tab launch superseded while sidecar was starting".into());
        }
        let busy = history.active_turn.is_some();
        let mut control = super::control::ControlState::default();
        control.restore(
            &thread,
            history.active_turn.as_deref(),
            &history.completed_turns,
        );
        let remote_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| error.to_string())?;
        let remote_port = remote_listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let sidecar = Arc::new(Sidecar {
            normalizer: Mutex::new(history.normalizer(&thread)),
            auth: Mutex::new(auth),
            controls: Mutex::new(super::model::TurnControls {
                model: spawn.model.clone(),
                effort: params["config"]["model_reasoning_effort"]
                    .as_str()
                    .map(str::to_owned),
                mode: runtime_mode,
                ..Default::default()
            }),
            control: Mutex::new(control),
            compaction: Mutex::new(super::compaction::CompactionState::default()),
            turn_settled: tokio::sync::watch::channel(0).0,
            busy: std::sync::atomic::AtomicBool::new(busy),
            client,
            port: remote_port,
            upstream_port: port,
            binary,
            version: init.user_agent,
            thread: Mutex::new(thread.clone()),
            verified: std::sync::atomic::AtomicBool::new(true),
            token,
            thread_settings: params,
            process,
            launch_nonce: attach_nonce.clone(),
        });
        crate::service::structured_endpoint::register(
            tab,
            crate::service::structured_endpoint::Endpoint::Rpc(sidecar.clone()),
        );
        if let Some(tab) = svc.mux.write().get_tab_mut(tab) {
            tab.structured_control_active = true;
            tab.structured_runtime_mode = serde_json::to_value(runtime_mode)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned));
        }
        svc.chat_registry.shutdown_tab(tab);
        bind(svc, tab, thread.clone());
        super::status::observe(
            svc,
            tab,
            &thread,
            &sidecar,
            if busy {
                "turn/started"
            } else {
                "turn/completed"
            },
        );
        super::subscriber::start(svc.clone(), tab.into(), &sidecar);
        log::info!("[codex-rpc-attach] tab={tab} nonce={attach_nonce} upstream_port={port} remote_port={remote_port} state=pending");
        super::attachment::start(
            svc.clone(),
            tab.into(),
            attach_nonce,
            thread,
            sidecar.token.clone(),
            port,
            remote_listener,
            sidecar.process.death.clone(),
        );
        if resume.is_none() {
            super::thread_start::refresh_account(svc.clone(), tab.into(), sidecar.clone());
        }
        crate::service::launch_diagnostics::log_rpc_sidecar(tab);
        artifacts.committed = true;
        client_guard.committed = true;
        Ok(sidecar)
    }
    .await;
    if result.is_err() {
        cleanup.finish().await?;
    }
    result
}

struct StartupClient {
    client: Client,
    committed: bool,
}

impl Drop for StartupClient {
    fn drop(&mut self) {
        if !self.committed {
            self.client.close("Sidecar startup failed");
        }
    }
}

struct Artifacts {
    token: std::path::PathBuf,
    marker: std::path::PathBuf,
    committed: bool,
}
impl Drop for Artifacts {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.token);
            let _ = std::fs::remove_file(&self.marker);
        }
    }
}

pub fn bind(svc: &ServiceContext, tab_id: &str, thread: String) {
    svc.set_agent_session_id(tab_id, Some(thread.clone()));
    let proof = {
        let mut mux = svc.mux.write();
        mux.get_tab_mut(tab_id).and_then(|tab| {
            if tab.agent_session_id.as_deref() != Some(&thread) {
                return None;
            }
            tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
            Some(crate::service::orchestrator::ProviderSessionBinding {
                pty_id: tab.pty_id.clone()?,
                binding_nonce: tab.session_binding_nonce.clone()?,
            })
        })
    };
    if let Some(proof) = proof {
        crate::service::orchestrator::record_provider_session(tab_id, &proof);
    }
}

pub async fn resume_thread(client: &Client, params: Value) -> Result<Value, String> {
    match client.request("thread/resume", params.clone()).await {
        Ok(result) => Ok(result),
        Err(ClientError::Rpc(error)) if archived(&error.message) => {
            client
                .request("thread/unarchive", json!({"threadId":params["threadId"]}))
                .await
                .map_err(rpc_error)?;
            client
                .request("thread/resume", params)
                .await
                .map_err(rpc_error)
        }
        Err(error) => Err(rpc_error(error)),
    }
}

pub fn archived(message: &str) -> bool {
    message.contains("codex unarchive")
        || (message.contains("session ") && message.contains(" is archived"))
}

pub fn rpc_error(error: ClientError) -> String {
    format!("App-server request failed: {error:?}")
}

fn write_token(path: &std::path::Path, token: &str) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(token.as_bytes()))
        .map_err(|error| format!("Sidecar token write {path:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_lifecycle_tokens_are_tab_scoped_and_file_is_exclusive() {
        let a = token_for_tab("token-test-a");
        assert_eq!(a, token_for_tab("token-test-a"));
        assert_ne!(a, token_for_tab("token-test-b"));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("token");
        write_token(&file, &a).unwrap();
        let error = write_token(&file, "replacement").unwrap_err();
        assert!(error.contains(&format!("{file:?}")), "{error}");
        assert_eq!(std::fs::read_to_string(file).unwrap(), a);
    }
    #[test]
    fn codex_rpc_lifecycle_failed_start_removes_private_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("owned.token");
        let marker = dir.path().join("owned.json");
        std::fs::write(&token, "opaque").unwrap();
        std::fs::write(&marker, "{}").unwrap();
        drop(Artifacts {
            token: token.clone(),
            marker: marker.clone(),
            committed: false,
        });
        assert!(!token.exists());
        assert!(!marker.exists());
    }
    #[test]
    fn codex_rpc_restore_unarchive_matches_only_archived_errors() {
        assert!(archived("session abc is archived"));
        assert!(archived("use codex unarchive"));
        assert!(!archived("thread not found"));
        assert!(!archived("permission denied"));
    }
}
