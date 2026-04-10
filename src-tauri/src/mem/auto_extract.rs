//! Auto-extraction: on save, spawn a hidden graph-worker that uses
//! `claude --model haiku` to extract entities and relations from text,
//! then writes them back via `codeterm mem` CLI commands.
//!
//! Completely free (OAuth, no API key). Worker is visible in the Memory
//! Panel workers tab as a log of what was extracted.
//!
//! The worker pane is created headless — PTY spawned from backend with a
//! backend reader, hidden from the start. Never flashes in the main tab bar.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::Emitter;

use crate::service::ServiceContext;
use crate::mem::workers;
use crate::mem::store::MemKind;

/// Kinds that trigger auto-extraction on save.
/// Facts and notes contain rich content worth extracting entities from.
/// Preferences and rules are short, structured — skip extraction.
/// Skills have their own indexing path.
fn should_extract(kind: MemKind) -> bool {
    matches!(
        kind,
        MemKind::Conversation | MemKind::Note | MemKind::Daily | MemKind::Learning | MemKind::Fact
    )
}

/// Minimum body length to justify spawning a worker.
const MIN_BODY_LEN: usize = 80;

/// Build the extraction prompt for haiku.
fn build_extraction_prompt(mem_item_id: &str, title: &str, body: &str) -> String {
    let truncated = if body.len() > 4000 { &body[..4000] } else { body };

    format!(
        r#"Extract people, projects, decisions, and area from this text. Execute the CLI commands below.

SOURCE: id="{id}", title="{title}"

TEXT:
---
{body}
---

STEP 1 — CLASSIFY AREA first (needed for steps below):
Determine the primary project or topic. Use a short lowercase name (rekap, soundcalm, deploy, auth, infra, home).
Run: codeterm mem update {id} --area <area>

STEP 2 — PEOPLE (only named individuals, not roles):
For each person mentioned by name:
  codeterm mem graph create --id person_<name> --entity-type Person --scope global
  codeterm mem graph relate --from {id} --rel mentions --to person_<name>

STEP 3 — PROJECTS (only named projects/repos/products):
For each project mentioned by name:
  codeterm mem graph create --id project_<name> --entity-type Project --scope global
  codeterm mem graph relate --from {id} --rel mentions --to project_<name>

STEP 4 — DECISIONS (explicit "we decided", "let's use", "better to"):
For each clear decision:
  codeterm mem save --kind fact --title "decision: <short>" --tags auto-extracted,decision --area <area> --importance 2 "<what was decided and why>"

STEP 5 — PROBLEMS (explicit "bug in", "doesn't work", "the issue was"):
For each clear problem:
  codeterm mem save --kind learning --title "problem: <short>" --tags auto-extracted,problem --area <area> --importance 2 "<problem and fix if mentioned>"

STEP 6 — Report:
  codeterm mem worker report "N people, M projects, K decisions, area=<area>"

RULES:
- Do NOT create technology entities — technologies are searchable via FTS in the body text.
- Only extract what's clearly stated. Don't infer or guess.
- IDs: lowercase_underscore. "John Smith" → person_john_smith, "Rekap" → project_rekap.
- Skip generic terms without a specific name.
- Use canonical names: "Tima" and "Timofey" → person_tima (pick the short form).
- `graph create` is idempotent — safe to call even if entity already exists.
- Decisions must be specific: "use PostgreSQL" is too vague. "Use PgBouncer transaction mode with max_client_conn=400" is good.
- If nothing to extract, report "nothing to extract" and exit.

Execute now."#,
        id = mem_item_id,
        title = title,
        body = truncated,
    )
}

/// Spawn a fully headless extraction worker. Uses the same pattern as
/// General Agent: backend PTY spawn + backend reader + send launch command.
/// The pane is hidden from birth — zero tab flash.
pub fn spawn_extraction_worker(
    svc: &Arc<ServiceContext>,
    mem_item_id: String,
    title: String,
    body: String,
    kind: MemKind,
) {
    if !should_extract(kind) || body.len() < MIN_BODY_LEN {
        return;
    }

    let worker_id = workers::new_worker_id();
    let intent = format!("extract:{}", &mem_item_id);
    let prompt = build_extraction_prompt(&mem_item_id, &title, &body);
    let task = workers::build_worker_task(&intent, &worker_id, &prompt);

    // 1. Find workspace
    let ws_id = {
        let mux = svc.mux.lock();
        mux.list_workspaces().first().map(|w| w.id.clone())
    };
    let Some(ws_id) = ws_id else {
        log::warn!("[auto-extract] no workspace, skipping {}", mem_item_id);
        return;
    };

    let ws_cwd = {
        let mux = svc.mux.lock();
        mux.get_workspace(&ws_id).and_then(|ws| ws.cwd.clone())
    };

    // 2. Create tab + pane, immediately hidden
    let (_tab_id, pane_id) = {
        let mut mux = svc.mux.lock();
        match mux.create_tab(&ws_id) {
            Ok(result) => {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.hidden = true;
                    pane.show_in_tabbar = false;
                    pane.worker_id = Some(worker_id.clone());
                    pane.auto_approve = true;
                }
                result
            }
            Err(e) => {
                log::warn!("[auto-extract] create_tab: {}", e);
                return;
            }
        }
    };

    // 3. Spawn PTY headless
    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("CODETERM_PANE_ID".to_string(), pane_id.clone());
    env_vars.insert("CODETERM_WORKER_ID".to_string(), worker_id.clone());
    env_vars.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );

    let pty_id = {
        let mut pty = svc.pty.lock();
        match pty.spawn(None, ws_cwd, 120, 40, env_vars) {
            Ok((id, _)) => id,
            Err(e) => {
                log::warn!("[auto-extract] PTY spawn: {}", e);
                return;
            }
        }
    };

    // 4. Link PTY to pane
    {
        let mut mux = svc.mux.lock();
        mux.link_pty(&pane_id, pty_id.clone());
    }
    svc.emit_state_changed();

    // 5. Resolve extraction config before registering worker (so we can record provider/model)
    let mem_cfg = crate::config::memory_config();
    let agent_provider = if mem_cfg.extraction_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.extraction_agent.clone()
    };
    let extraction_model = mem_cfg.extraction_model.clone();
    let agent_registry = svc.agent_registry.clone();
    let model_args: Vec<String> = agent_registry
        .with_provider(&agent_provider, |p| p.model_launch_args(&extraction_model))
        .unwrap_or_default();

    // Register worker with provider/model metadata
    let color = svc.workers.pick_color();
    let mut info = workers::make_info(
        worker_id.clone(),
        pane_id.clone(),
        intent,
        "auto:save".into(),
        color,
    );
    info.agent_provider = Some(agent_provider.clone());
    if !extraction_model.is_empty() {
        info.model = Some(extraction_model);
    }
    svc.workers.register(info.clone());
    let _ = svc.app_handle.emit("graph-worker-spawned", &info);

    log::info!(
        "[auto-extract] headless worker {} pane={} pty={} for {}",
        worker_id, pane_id, pty_id, mem_item_id
    );

    // 6. Backend reader + launch agent — all in a background thread

    let svc2 = svc.clone();
    let pane_id2 = pane_id.clone();
    let pty_id2 = pty_id.clone();
    std::thread::spawn(move || {
        // Start backend reader (needs its own tokio runtime)
        let svc3 = svc2.clone();
        let pty_id3 = pty_id2.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(&svc3, &pty_id3) {
                    Ok(handle) => { let _ = handle.await; }
                    Err(e) => log::warn!("[auto-extract] backend reader: {}", e),
                }
            });
        });

        // Wait for shell prompt
        crate::service::pty_io::poll_shell_prompt(
            &svc2.screen_state,
            &pty_id2,
            std::time::Duration::from_secs(10),
        );

        // Build launch command using the configured agent provider
        let launch_params = crate::agent::provider::AgentLaunchParams {
            agent_name: None,
            task: Some(task),
            args: if model_args.is_empty() { None } else { Some(model_args) },
            skip_permissions: true,
        };
        let launch_cmd = agent_registry
            .with_provider(&agent_provider, |p| p.build_launch_command(&launch_params))
            .unwrap_or_else(|| format!("{} --dangerously-skip-permissions", agent_provider));

        if let Err(e) = svc2.send_text(&pane_id2, &launch_cmd, true) {
            log::error!("[auto-extract] send launch cmd: {}", e);
        }
        log::debug!(
            "[auto-extract] agent launched in pane {} provider={}",
            pane_id2, agent_provider
        );
    });
}
