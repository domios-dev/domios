import { PageDataContext } from "./PageDataContext";
import { useState, useEffect, useCallback, useRef, useContext } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import { webRequestContext, assertWebRequestContext } from "../lib/webApi";
import type { AppConfig } from "../lib/types";
import { DEFAULT_THEME, readCachedTheme } from "../lib/themeCss";

export function defaultConfig(): AppConfig {
  return {
    browser_upload_folders: {},
    restore: { active_window_hours: 48 },
    tab_sleep: { enabled: true, idle_minutes: 60 },
    account: {},
    billing: {},
    plugins: {},
    utility_tabs: { max_per_team: { build: null, dev: null, qa: null, deploy: null, logs: null, db: null, shell: null } },
    usage: { enabled: true, heartbeat: true, freshness_secs: 180, standalone_cadence_secs: 0 },
    updater: { channel: "stable", allow_provider_updates_in_non_prod: false, auto_update_provider_clis: true },
    runaway: { enabled: true, test_binary_max_secs: 900 },
    kpi: { enabled: true, default_applied: true },
    model_defaults: { replaced: {} },
    neighbors: { enabled: true },
    theme: readCachedTheme() ?? DEFAULT_THEME,
    font: {
      family: "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace",
      size: 14,
      ligatures: true,
    },
    terminal: {
      default_shell: null,
      cursor_style: "block",
      cursor_blink: true,
      scrollback: 10000,
      copy_on_select: false,
    },
    diagnostics: {
      terminal_transport: false,
      protocol_recording: false,
    },
    notifications: {
      // Off until someone opts in; until the daemon answers, nothing interrupts.
      desktop: false,
      sound: false,
      toast_duration: 5,
      // The bell has always been drawn; until the daemon answers, it stays drawn.
      show_bell: true,
      // The daemon owns which channels exist; until it answers there are none.
      channels: {},
      // The daemon owns the level set; until it answers there is nothing to
      // claim the user controls.
      agent_levels: [],
    },
    keybindings: {
      new_tab: "Ctrl+T", close_tab: "Ctrl+W", prev_tab: "Ctrl+Shift+Tab", new_workspace: "Ctrl+Shift+N",
      copy: "Ctrl+Shift+C", paste: "Ctrl+Shift+V", search: "Ctrl+Shift+F", zoom_in: "Ctrl+=", zoom_out: "Ctrl+-", zoom_reset: "Ctrl+0", open_settings: "Ctrl+,",
      reopen_tab: "Ctrl+Shift+T",
      next_tab: "Ctrl+Tab",
    },
    socket: { path: null },
    agent: {
      reply_relay_max_hops: 8,
      continue_after_restart: true,
      sandbox_ceiling: null,
      poll_interval_ms: 500,
      idle_timeout_s: 30,
      resume_sessions: true,
    },
    api: {
      enabled: true,
      host: "127.0.0.1",
      port: 7685,
      token: null,
      cors: false,
      cors_allow_origins: [],
      preview_secret_paths: [],
      mesh_tailnet_host: null,
    },
    agents: {},
    general_agent: {
      capability_hints: true,
      model: null,
      reasoning_effort: null,
      enabled: true,
      agent_provider: "claude",
      auto_restart: true,
      max_restart_attempts: 3,
      restart_delay_s: 60,
      first_run_complete: false,
    },
    memory: {
      backend: "sqlite", workers_per_provider: 3, dreaming_timeout_seconds: 1800, librarian_enabled: true,
      l1_budget_tokens: 300, dreaming_telegram_notify: true,
      dream_hygiene: { stale_archive: false, stale_archive_cap: 20, merge_cap: 10 },
      embedder: { provider: "local", model: "", dimension: 0, remote_opt_in: false },
      extraction_agent: "",
      extraction_model: "",
      auto_extract: true,
      dreaming_agent: "",
      dreaming_model: "",
      worker_model: "",
      worker_timeout_seconds: 1800,
      dedup_threshold: 0.15,
      file_sync_enabled: true,
      file_sync_interval_minutes: 60,
    },
    supporter: {
      connections: {},
      deadline_ms: 3000,
      calls_per_hour: null,
      memory_rerank: {
        docs_hybrid: false,
        sources: { recall: 30, graph: 3, docs: 3, sessions: 0 },
        candidates: 50,
        max_concurrency: 16,
        request_mode: "per_candidate",
        low_confidence_threshold: 0.15,
        max_body_chars: 2000,
      },
    },
    integrations: {
      voice_transcribe: { enabled: false, daemon_url: "http://127.0.0.1:7891", timeout_ms: 60000, language: "", fallback_raw_on_error: true, health_check_ttl_secs: 60, active: "transcriber" },
      text_hooks: { enabled: false, daemon_url: "http://127.0.0.1:7892", timeout_ms: 2000, health_check_ttl_secs: 60, fallback_on_error: "forward" },
      telegram_enabled: false,
      telegram_allowed_users: [],
      discord_enabled: false,
      discord_allowed_users: [],
      webhook_enabled: false,
      webhook_port: 9090,
      secrets: {
        backend: "bitwarden",
        bitwarden: { server_url: "https://vault.bitwarden.com" },
        default_organization: "",
        default_collection: "",
      },
    },
    mesh: {
      enabled: false,
      instance_name: "",
    },
    ui: {
      memory_tab_enabled: true,
      mesh_tab_enabled: true,
    },
    orchestration: {
      reflect: { every_gates: 0 }, digests: { enabled: false, rules: [] }, map: { enabled: false },
      refresh: { after_reflection: false, handoff_timeout_secs: 60, settle_timeout_secs: 120, reorient_timeout_secs: 30, verify_timeout_secs: 180 },
      plan_reminders: true,
      nudge_interval_secs: 1800,
    },
  };
}

function configEquals(a: AppConfig, b: AppConfig): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function mergeConfigEvent(previous: AppConfig, incoming: AppConfig): AppConfig {
  if (incoming.api.token || !previous.api.token) return incoming;
  return { ...incoming, api: { ...incoming.api, token: previous.api.token } };
}

export function useConfig() {
  const page = useContext(PageDataContext);
  const local = useConfigLoader(!page);
  return page?.config ?? local;
}

export function useConfigLoader(enabled = true) {
  const [generation, setGeneration] = useState(webRequestContext);
  const [snapshot, setSnapshot] = useState(() => ({
    generation, config: defaultConfig(), loading: true, ready: false,
  }));
  const readyGeneration = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const reset = () => {
      readyGeneration.current = null;
      setSnapshot({ generation: webRequestContext(), config: defaultConfig(), loading: true, ready: false });
    };
    const rebind = () => setGeneration(webRequestContext());
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    return () => {
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", rebind);
    };
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    readyGeneration.current = null;
    const current = () => !cancelled && generation === webRequestContext();
    setSnapshot({ generation, config: defaultConfig(), loading: true, ready: false });
    invoke<AppConfig>("get_config").then((config) => {
      if (!current()) return;
      if (!config) throw new Error("Config is unavailable");
      readyGeneration.current = generation;
      setSnapshot({ generation, config, loading: false, ready: true });
    }).catch(() => {
      if (current()) setSnapshot({ generation, config: defaultConfig(), loading: false, ready: false });
    });
    void listen<AppConfig>("config-changed", (event) => {
      if (!current() || readyGeneration.current !== generation) return;
      setSnapshot((previous) => {
        if (previous.generation !== generation || !previous.ready) return previous;
        const config = mergeConfigEvent(previous.config, event.payload);
        return configEquals(previous.config, config) ? previous : { ...previous, config };
      });
    }).then((fn) => {
      if (current()) unlisten = fn;
      else fn();
    }).catch(() => undefined);
    return () => {
      cancelled = true;
      readyGeneration.current = null;
      unlisten?.();
    };
  }, [generation, enabled]);

  const updateConfig = useCallback(async (newConfig: AppConfig) => {
    assertWebRequestContext(generation);
    if (!snapshot.ready || snapshot.generation !== generation || readyGeneration.current !== generation) {
      throw new Error("Config is not ready for this peer");
    }
    await invoke("update_config", { config: newConfig });
    assertWebRequestContext(generation);
  }, [generation, snapshot.ready, snapshot.generation]);

  const current = snapshot.generation === generation && generation === webRequestContext();
  const config = current ? snapshot.config : defaultConfig();
  return { config, updateConfig, theme: config.theme, loading: !current || snapshot.loading, ready: current && snapshot.ready };
}
