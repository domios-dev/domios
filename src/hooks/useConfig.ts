import { useState, useEffect, useCallback } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import type { AppConfig } from "../lib/types";
import { DEFAULT_THEME, readCachedTheme } from "../lib/themeCss";

export function defaultConfig(): AppConfig {
  return {
  theme: readCachedTheme() ?? DEFAULT_THEME,
  font: {
    family: "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace",
    size: 14,
    ligatures: true,
  },
  terminal: {
    cursor_style: "block",
    cursor_blink: true,
    scrollback: 10000,
    copy_on_select: false,
  },
  notifications: {
    desktop: true,
    sound: false,
    toast_duration: 5,
  },
  keybindings: {
    reopen_tab: "Ctrl+Shift+T",
  },
  socket: { path: null },
  agent: {
    poll_interval_ms: 500,
    idle_timeout_s: 30,
    resume_sessions: true,
    full_access: undefined,
  },
  api: {
    enabled: true,
    host: "127.0.0.1",
    port: 7685,
    token: null,
    cors: false,
    cors_allow_origins: [],
  },
  agents: {},
  general_agent: {
    enabled: true,
    agent_provider: "claude",
    auto_restart: true,
    max_restart_attempts: 3,
    restart_delay_s: 60,
    first_run_complete: false,
  },
  memory: {
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
  integrations: {
    telegram_enabled: false,
    telegram_allowed_users: [],
    discord_enabled: false,
    discord_allowed_users: [],
    webhook_enabled: false,
    webhook_port: 9090,
    secrets: {
      backend: "bitwarden",
      default_organization: "",
      default_collection: "",
    },
  },
  mesh: {
    enabled: false,
    instance_name: "",
    broadcast_duration_secs: 180,
  },
  ui: {
    memory_tab_enabled: true,
    mesh_tab_enabled: true,
  },
  orchestration: {
    plan_reminders: true,
    nudge_interval_secs: 1800,
  },
  };
}

function configEquals(a: AppConfig, b: AppConfig): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function useConfig() {
  const [config, setConfig] = useState<AppConfig>(() => defaultConfig());
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    invoke<AppConfig>("get_config")
      .then((cfg) => {
        if (!cancelled) {
          setConfig(cfg);
          setLoading(false);
        }
      })
      .catch((err) => {
        console.warn("Failed to load config from backend:", err);
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  // Listen for config-changed events (hot reload)
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    listen<AppConfig>("config-changed", (event) => {
      if (!cancelled) {
        setConfig((prev) => (configEquals(prev, event.payload) ? prev : event.payload));
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Update config via Rust backend (persists to disk + emits event)
  const updateConfig = useCallback(async (newConfig: AppConfig) => {
    try {
      await invoke("update_config", { config: newConfig });
    } catch (err) {
      console.error("Failed to update config:", err);
      throw err;
    }
  }, []);

  const theme = config.theme;

  return { config, updateConfig, theme, loading };
}
