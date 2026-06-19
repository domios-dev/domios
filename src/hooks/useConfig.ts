import { useState, useEffect, useCallback } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import type { AppConfig } from "../lib/types";
import { DEFAULT_THEME, readCachedTheme } from "../lib/themeCss";

function defaultConfig(): AppConfig {
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
    new_tab: "Ctrl+T",
    reopen_tab: "Ctrl+Shift+T",
    close_tab: "Ctrl+Shift+W",
    next_tab: "Ctrl+Tab",
    prev_tab: "Ctrl+Shift+Tab",
    close_pane: "Ctrl+Shift+Q",
    focus_pane_left: "Alt+Left",
    focus_pane_right: "Alt+Right",
    focus_pane_up: "Alt+Up",
    focus_pane_down: "Alt+Down",
    new_workspace: "Ctrl+Shift+N",
    copy: "Ctrl+Shift+C",
    paste: "Ctrl+Shift+V",
    search: "Ctrl+Shift+F",
    zoom_in: "Ctrl+=",
    zoom_out: "Ctrl+-",
    zoom_reset: "Ctrl+0",
    open_settings: "Ctrl+,",
  },
  socket: { path: null },
  agent: {
    poll_interval_ms: 500,
    idle_timeout_s: 30,
    resume_sessions: true,
    resume_skip_permissions: false,
  },
  api: {
    enabled: true,
    host: "0.0.0.0",
    port: 7685,
    token: null,
    cors: true,
  },
  agents: {},
  general_agent: {
    enabled: true,
    agent_provider: "claude",
    auto_restart: true,
    max_restart_attempts: 3,
    restart_delay_s: 60,
  },
  memory: {
    extraction_agent: "",
    extraction_model: "haiku",
    auto_extract: true,
    dreaming_agent: "",
    dreaming_model: "haiku",
    worker_model: "haiku",
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
    default_new_agent_pane_chat: false,
  },
  };
}

function configEquals(a: AppConfig, b: AppConfig): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function useConfig() {
  const [config, setConfig] = useState<AppConfig>(() => defaultConfig());
  const [loading, setLoading] = useState(true);

  // Fetch config from Rust backend on mount
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
      // Config will be updated via the config-changed event listener
    } catch (err) {
      console.error("Failed to update config:", err);
      throw err;
    }
  }, []);

  const theme = config.theme;

  return { config, updateConfig, theme, loading };
}
