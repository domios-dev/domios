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
    split_right: "Ctrl+Shift+D",
    split_down: "Ctrl+Shift+E",
    close_pane: "Ctrl+Shift+Q",
  },
  socket: { path: null },
  agent: {
    poll_interval_ms: 500,
    idle_timeout_s: 30,
    resume_sessions: true,
    resume_skip_permissions: false,
  },
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
