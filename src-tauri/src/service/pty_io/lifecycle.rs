//! PTY lifecycle helpers: pane env, ensure/materialize, resize, kill, resolve,
//! and wait helpers.
use super::{
    restored_agent_materialization, RestoredAgentMaterialization, SEND_AGENT_BACKEND_COLS,
    SHELL_BACKEND_COLS,
};
use crate::service::ServiceContext;
use std::collections::HashMap;

impl ServiceContext {
    pub(crate) fn pane_env(&self, pane_id: &str) -> HashMap<String, String> {
        let mut env_vars = HashMap::new();
        env_vars.insert("CODETERM_PANE_ID".into(), pane_id.to_string());
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(pane_id),
        );
        env_vars.insert(
            "CODETERM_SOCKET_PATH".into(),
            crate::socket::server::socket_display_path(),
        );
        let api_cfg = crate::config::load_config().api;
        let host = if api_cfg.host == "0.0.0.0" {
            "127.0.0.1".to_string()
        } else {
            api_cfg.host.clone()
        };
        env_vars.insert(
            "CODETERM_API_URL".into(),
            format!("http://{}:{}", host, api_cfg.port),
        );
        env_vars.insert("CODETERM_API_PORT".into(), api_cfg.port.to_string());
        if let Some(ref tok) = api_cfg.token {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }

        let mux = self.mux.read();
        if let Some(pane) = mux.get_pane(pane_id) {
            if let Some(ref role) = pane.role {
                env_vars.insert("CODETERM_ROLE".into(), format!("{:?}", role));
            }
            if let Some(ref parent) = pane.parent_pane_id {
                env_vars.insert("CODETERM_PARENT_PANE_ID".into(), parent.clone());
            }
            if let Some(ref worker_id) = pane.worker_id {
                env_vars.insert("CODETERM_WORKER_ID".into(), worker_id.clone());
            }
        }
        env_vars
    }

    pub(crate) fn start_backend_reader_for(&self, pty_id: String) {
        let svc_reader = std::sync::Arc::new(self.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_id,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => {
                        log::warn!(
                            "[ensure-pty] backend reader failed for {}: {}",
                            &pty_id[..pty_id.len().min(8)],
                            e
                        );
                    }
                }
            });
        });
    }

    pub(crate) fn restored_tab_context(&self, pane_id: &str) -> Option<(String, String)> {
        let mux = self.mux.read();
        for ws in &mux.snapshot().workspaces {
            for tab in &ws.tabs {
                if tab.pane_id == pane_id {
                    return Some((ws.id.clone(), tab.id.clone()));
                }
            }
        }
        None
    }

    pub(crate) fn ensure_pane_pty_for_send(&self, pane_id: &str) -> Result<(), String> {
        // [G] A chatBackend pane must NEVER spawn a shell PTY (B1.1#4). Its
        // session is opened by the spawn path; on send there is nothing to
        // ensure. Fixing this one function neutralizes all 9 callers.
        if self.pane_chat_backing(pane_id).is_some() {
            return Ok(());
        }
        let resolved = self
            .mux
            .read()
            .resolve_pane_id(pane_id)
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        if self.resolve_pty_id(&resolved).is_ok() {
            self.materialize_restored_agent_for_send(&resolved)?;
            return Ok(());
        }

        let (cwd, protected, agent_type) = {
            let mux = self.mux.read();
            let pane = mux
                .get_pane(&resolved)
                .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
            (pane.cwd.clone(), pane.protected, pane.agent_type.clone())
        };
        if protected {
            return Err(format!(
                "Pane {} is protected and its backend PTY is not live yet",
                pane_id
            ));
        }

        let env_vars = self.pane_env(&resolved);
        let cols = if agent_type.is_some() {
            SEND_AGENT_BACKEND_COLS
        } else {
            SHELL_BACKEND_COLS
        };
        let t0 = std::time::Instant::now();
        let built = crate::pty::manager::build_session(None, cwd.clone(), cols, 40, env_vars)?;
        let (pty_id, _) = self.pty.lock().register_session(None, built);
        log::info!(
            "[ensure-pty] pane {} activated backend pty {} (build={}ms)",
            resolved,
            &pty_id[..pty_id.len().min(8)],
            t0.elapsed().as_millis()
        );
        self.link_pty(&resolved, pty_id.clone());
        self.start_backend_reader_for(pty_id);

        self.materialize_restored_agent_for_send(&resolved)?;
        Ok(())
    }

    pub(crate) fn materialize_restored_agent_for_send(&self, pane_id: &str) -> Result<(), String> {
        let (provider_id, session_id, cwd, protected, restored, model) = {
            let mux = self.mux.read();
            let pane = mux
                .get_pane(pane_id)
                .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
            (
                pane.agent_type.clone(),
                pane.agent_session_id.clone(),
                pane.cwd.clone(),
                pane.protected,
                pane.restored_from_snapshot,
                pane.agent_session_model.clone(),
            )
        };
        let Some(provider_id) = provider_id else {
            return Ok(());
        };
        let session_valid = session_id
            .as_deref()
            .zip(cwd.as_deref())
            .map(|(sid, dir)| {
                self.agent_registry
                    .with_provider(&provider_id, |p| p.session_exists(dir, sid))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let Some(mode) = restored_agent_materialization(
            restored,
            protected,
            Some(&provider_id),
            session_id.as_deref(),
            session_valid,
        ) else {
            return Ok(());
        };
        let (ws_id, tab_id) = self.restored_tab_context(pane_id).ok_or_else(|| {
            format!(
                "Pane {} is a restored agent pane but has no tab context for materialization",
                pane_id
            )
        })?;

        let pc = crate::config::provider_config(&provider_id);
        let skip_permissions = pc.resume_skip_permissions.unwrap_or(true);
        let launch_session_id = match mode {
            RestoredAgentMaterialization::Resume => session_id.clone(),
            RestoredAgentMaterialization::Fresh => {
                if session_id.is_some() {
                    self.set_agent_session_id(pane_id, None);
                }
                None
            }
        };
        let params = crate::service::SpawnAgentParams {
            agent: Some(provider_id.clone()),
            cwd,
            skip_permissions: Some(skip_permissions),
            model,
            resume_session_id: launch_session_id,
            ..Default::default()
        };
        let launch_cmd = crate::service::orchestrator::build_agent_command(
            &provider_id,
            &params,
            None,
            None,
            &self.agent_registry,
        );
        crate::service::orchestrator::launch_agent_in_background(
            self.clone(),
            pane_id.to_string(),
            tab_id,
            ws_id,
            launch_cmd,
            provider_id,
            None,
            None,
        );
        self.clear_pane_restored_flag(pane_id);
        self.emit_state_changed();
        Ok(())
    }

    pub fn resize_pane(&self, pane_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        // A chatBackend pane has no terminal to resize — graceful no-op.
        if self.pane_chat_backing(pane_id).is_some() {
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .resize(&pty_id, cols, rows)
            .map_err(|e| e.to_string())
    }

    /// Kill the PTY process for a pane without removing the pane from the mux.
    /// Different from `close_pane`, which both kills the PTY and removes the pane.
    pub fn kill_pane_pty(&self, pane_id: &str) -> Result<(), String> {
        // A chatBackend pane has no PTY process — close the plugin session.
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            return self.chatbackend_close(&plugin_id, &session_id);
        }
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty.lock().remove(&pty_id);
        Ok(())
    }

    /// Resolve pane_id (or alias) → pty_id.
    pub fn resolve_pty_id(&self, pane_id: &str) -> Result<String, String> {
        let mux = self.mux.read();
        let resolved = mux
            .resolve_pane_id(pane_id)
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        let pty_id = mux
            .get_pane(&resolved)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        drop(mux);
        if self.pty.lock().get_session(&pty_id).is_none() {
            return Err(format!("Pane {} PTY is not live: {}", pane_id, pty_id));
        }
        Ok(pty_id)
    }

    /// Wait for a pane to get a PTY assigned by the frontend. Polls with timeout.
    pub async fn wait_for_pty(&self, pane_id: &str, timeout_secs: u64) -> Result<String, String> {
        let mux = self.mux.clone();
        let pid = pane_id.to_string();
        tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            loop {
                {
                    let m = mux.read();
                    if let Some(pane) = m.get_pane(&pid) {
                        if let Some(ref pty_id) = pane.pty_id {
                            return pty_id.clone();
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| "Timeout waiting for PTY".to_string())
    }

    /// Wait for output buffer to grow by at least `min_bytes`.
    pub async fn wait_for_output_growth(&self, pty_id: &str, min_bytes: usize, timeout_secs: u64) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async move {
            let start_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
            loop {
                let cur_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
                if cur_len > start_len + min_bytes {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        })
        .await;
    }
}
