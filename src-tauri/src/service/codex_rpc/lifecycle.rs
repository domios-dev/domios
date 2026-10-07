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

pub struct Sidecar {
    pub client: Client,
    pub controls: Mutex<super::model::TurnControls>,
    pub control: Mutex<super::control::ControlState>,
    pub compaction: Mutex<super::compaction::CompactionState>,
    pub turn_settled: tokio::sync::watch::Sender<u64>,
    pub port: u16,
    pub binary: std::path::PathBuf,
    pub version: String,
    pub thread: Mutex<String>,
    pub verified: std::sync::atomic::AtomicBool,
    pub busy: std::sync::atomic::AtomicBool,
    pub auth: Mutex<super::auth::AuthState>,
    token: String,
    pub thread_settings: Value,
    pub process: Process,
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
    if provider
        .as_deref()
        .is_some_and(|provider| super::launch::requested(svc, provider))
    {
        env.insert("CODETERM_CODEX_RPC_TOKEN".into(), token_for_tab(tab));
    }
}

pub fn for_tab(tab: &str) -> Option<Arc<Sidecar>> {
    crate::service::structured_endpoint::rpc(tab)
}
pub fn clear_tab(tab: &str) {
    let sidecar = for_tab(tab);
    if let Some(sidecar) = sidecar {
        super::approvals::clear_tab(tab);
        crate::service::structured_endpoint::remove(tab);
        sidecar.client.close("Tab lifecycle ended");
        sidecar.process.stop();
    }
}

pub fn extend_env(tab: &str, env: &mut HashMap<String, String>) {
    if let Some(sidecar) = for_tab(tab) {
        env.insert("CODETERM_CODEX_RPC_TOKEN".into(), sidecar.token.clone());
    }
}

pub struct Spawn {
    pub provider: String,
    pub cwd: String,
    pub instructions: Option<String>,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub full_access: bool,
}

pub async fn ensure_for_spawn(
    svc: &ServiceContext,
    tab: &str,
    spawn: Spawn,
) -> Result<Arc<Sidecar>, String> {
    let gate = STARTING
        .lock()
        .entry(tab.into())
        .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let _permit = gate.acquire().await.map_err(|error| error.to_string())?;
    let previous = for_tab(tab);
    clear_tab(tab);
    if let Some(previous) = previous {
        let mut death = previous.process.death.clone();
        if !*death.borrow() {
            tokio::time::timeout(std::time::Duration::from_secs(5), death.changed())
                .await
                .map_err(|_| "Previous sidecar termination deadline")?
                .map_err(|_| "Previous sidecar termination channel closed")?;
        }
    }
    let (cap, binary) = svc
        .agent_registry
        .with_provider(&spawn.provider, |provider| {
            (
                provider.structured_control(),
                codeterm_plugin::js_runtime::resolved_bin_path(provider.binary_name()),
            )
        })
        .ok_or("Provider unavailable")?;
    let cap = cap.ok_or("Structured control unavailable")?;
    let binary = binary.ok_or("Provider binary unavailable")?;
    if !binary.is_absolute() {
        return Err("Sidecar binary must be absolute".into());
    }
    if matches!(
        binary.extension().and_then(|ext| ext.to_str()),
        Some("cmd" | "bat")
    ) {
        return Err("Sidecar requires a native executable; plain PTY fallback selected".into());
    }
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    let root = crate::paths::data_dir().join("codex-rpc");
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
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
    let client = Client::connect(port, &token).await?;
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
        .and_then(|mode| serde_json::from_value::<super::model::RuntimeMode>(json!(mode)).ok())
        .unwrap_or(if spawn.full_access {
            super::model::RuntimeMode::FullAccess
        } else {
            super::model::RuntimeMode::AutoAcceptEdits
        });
    let mut params = json!({"cwd":spawn.cwd,"developerInstructions":spawn.instructions,
        "model":spawn.model,"config":{},
        "approvalPolicy":if spawn.full_access {"never"} else {"on-request"},
        "approvalsReviewer":"user", "sandbox":if spawn.full_access {"danger-full-access"} else {"workspace-write"}});
    let overrides = runtime_mode.overrides();
    params["approvalPolicy"] = overrides["approvalPolicy"].clone();
    params["approvalsReviewer"] = overrides["approvalsReviewer"].clone();
    params["sandbox"] = json!(match runtime_mode {
        super::model::RuntimeMode::ApprovalRequired => "read-only",
        super::model::RuntimeMode::FullAccess => "danger-full-access",
        _ => "workspace-write",
    });
    if let Some(effort) = spawn.effort {
        params["config"]["model_reasoning_effort"] = json!(effort);
    }
    let resume = spawn
        .resume
        .filter(|_| crate::config::resume_sessions_enabled(&spawn.provider));
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
    let response: super::schema::ThreadResponse =
        serde_json::from_value(response).map_err(|error| error.to_string())?;
    let thread = response.thread.id;
    if resume.is_none() {
        client.request("thread/inject_items", json!({"threadId":thread,"items":[{
            "type":"message","role":"developer","content":[{"type":"input_text","text":"Domios owns this interactive terminal session."}]
        }]})).await.map_err(rpc_error)?;
    }
    let read = client
        .request(
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        )
        .await
        .map_err(rpc_error)?;
    if !super::routing::verified_user_thread(&thread, &read) {
        client.close("Thread identity unverified");
        return Err("Sidecar thread identity unverified".into());
    }
    let auth = client
        .request("account/read", json!({"refreshToken":false}))
        .await
        .map(|response| super::auth::account_state(&response))
        .unwrap_or_default();
    if !svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.session_binding_nonce == launch_nonce)
    {
        client.close("Tab launch superseded");
        return Err("Tab launch superseded while sidecar was starting".into());
    }
    let sidecar = Arc::new(Sidecar {
        auth: Mutex::new(auth),
        controls: Mutex::new(super::model::TurnControls {
            model: spawn.model.clone(),
            effort: params["config"]["model_reasoning_effort"]
                .as_str()
                .map(str::to_owned),
            mode: runtime_mode,
        }),
        control: Mutex::new(super::control::ControlState::default()),
        compaction: Mutex::new(super::compaction::CompactionState::default()),
        turn_settled: tokio::sync::watch::channel(0).0,
        busy: std::sync::atomic::AtomicBool::new(false),
        client,
        port,
        binary,
        version: init.user_agent,
        thread: Mutex::new(thread.clone()),
        verified: std::sync::atomic::AtomicBool::new(true),
        token,
        thread_settings: params,
        process,
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
    bind(svc, tab, thread);
    super::subscriber::start(svc.clone(), tab.into(), &sidecar);
    crate::service::launch_diagnostics::log_rpc_sidecar(tab);
    artifacts.committed = true;
    Ok(sidecar)
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
        .map_err(|error| error.to_string())
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
        assert!(write_token(&file, "replacement").is_err());
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
