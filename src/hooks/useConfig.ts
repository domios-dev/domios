import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, ThemeConfig } from "../lib/types";

// Default theme (GitHub Dark) used before config loads from backend
const DEFAULT_THEME: ThemeConfig = {
  name: "github-dark",
  background: "#0d1117",
  foreground: "#c9d1d9",
  cursor: "#58a6ff",
  selection: "#264f78",
  black: "#0d1117",
  red: "#ff7b72",
  green: "#7ee787",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39d353",
  white: "#c9d1d9",
  bright_black: "#484f58",
  bright_red: "#ffa198",
  bright_green: "#56d364",
  bright_yellow: "#e3b341",
  bright_blue: "#79c0ff",
  bright_magenta: "#d2a8ff",
  bright_cyan: "#56d364",
  bright_white: "#f0f6fc",
};

const DEFAULT_CONFIG: AppConfig = {
  theme: DEFAULT_THEME,
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
    new_tab: "Ctrl+Shift+T",
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
    open_browser: "Ctrl+Shift+B",
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
    worker_model: "claude-haiku-4-5-20251001",
    worker_timeout_seconds: 150,
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
  onboarding_completed: false,
};

export function useConfig() {
  const [config, setConfig] = useState<AppConfig>(DEFAULT_CONFIG);
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
        setConfig(event.payload);
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
