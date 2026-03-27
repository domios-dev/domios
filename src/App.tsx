import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Titlebar } from "./components/Titlebar/Titlebar";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { SplitContainer } from "./components/Terminal/SplitContainer";
import { NotificationToastContainer } from "./components/Notifications/NotificationToast";
import { SettingsPanel } from "./components/Settings/SettingsPanel";
import { Onboarding } from "./components/Onboarding/Onboarding";
import { CommandPalette, buildDefaultCommands } from "./components/CommandPalette/CommandPalette";
import { useGitInfo } from "./hooks/useGitInfo";
import { useConfig } from "./hooks/useConfig";
import {
  MultiplexerProvider,
  useMultiplexer,
} from "./store/multiplexer";
import { CronProvider } from "./store/crons";
import type { ThemeConfig } from "./lib/types";
import { matchesBinding, isMac } from "./lib/keybindings";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
// TODO: Agent Swarm Visualization — future feature
// import { SwarmOverlay } from "./components/Swarm/SwarmOverlay";

/**
 * Derive CSS custom properties from a ThemeConfig so the entire UI
 * can reference them for consistent theming.
 */
function themeToCSS(theme: ThemeConfig): Record<string, string> {
  return {
    "--ct-bg": theme.background,
    "--ct-fg": theme.foreground,
    "--ct-cursor": theme.cursor,
    "--ct-selection": theme.selection,
    "--ct-sidebar-bg": deriveColor(theme.background, 8),
    "--ct-titlebar-bg": deriveColor(theme.background, 8),
    "--ct-border": blendColor(theme.background, theme.bright_black, 0.38),
    "--ct-hover": deriveColor(theme.background, 14),
    "--ct-active": deriveColor(theme.background, 22),
    "--ct-surface": deriveColor(theme.background, 10),
    "--ct-muted": theme.bright_black,
    "--ct-muted-fg": blendColor(theme.bright_black, theme.foreground, 0.45),
    "--ct-accent": theme.blue,
    "--ct-red": theme.red,
    "--ct-green": theme.green,
    "--ct-yellow": theme.yellow,
    "--ct-orange": theme.red,
    ...(theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy" ? { "--ct-glow": theme.green + "99" } : {}),
  };
}

/**
 * Blend two hex colors by a ratio (0 = all colorA, 1 = all colorB).
 * Produces intentional tints instead of flat brightness shifts.
 */
function blendColor(hexA: string, hexB: string, ratio: number): string {
  const a = parseHex(hexA), b = parseHex(hexB);
  const r = Math.round(a[0] + (b[0] - a[0]) * ratio);
  const g = Math.round(a[1] + (b[1] - a[1]) * ratio);
  const bl = Math.round(a[2] + (b[2] - a[2]) * ratio);
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${bl.toString(16).padStart(2, "0")}`;
}

function parseHex(hex: string): [number, number, number] {
  const c = hex.replace("#", "");
  return [parseInt(c.substring(0, 2), 16), parseInt(c.substring(2, 4), 16), parseInt(c.substring(4, 6), 16)];
}

/**
 * Derive a color from a base by adjusting brightness.
 * Automatically lightens dark colors and darkens light ones.
 */
/** Derive color in HSL space — preserves hue instead of drifting to gray. */
function deriveColor(hex: string, amount: number): string {
  const [r, g, b] = parseHex(hex);
  const [h, s, l] = rgbToHsl(r, g, b);
  const brightness = (r + g + b) / 3;
  const dir = brightness > 128 ? -1 : 1;
  const nl = Math.min(100, Math.max(0, l + amount * dir * 0.4));
  const [nr, ng, nb] = hslToRgb(h, s, nl);
  return `#${nr.toString(16).padStart(2, "0")}${ng.toString(16).padStart(2, "0")}${nb.toString(16).padStart(2, "0")}`;
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l * 100];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return [h * 360, s * 100, l * 100];
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  h /= 360; s /= 100; l /= 100;
  if (s === 0) { const v = Math.round(l * 255); return [v, v, v]; }
  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0) t += 1; if (t > 1) t -= 1;
    if (t < 1/6) return p + (q - p) * 6 * t;
    if (t < 1/2) return q;
    if (t < 2/3) return p + (q - p) * (2/3 - t) * 6;
    return p;
  };
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [
    Math.round(hue2rgb(p, q, h + 1/3) * 255),
    Math.round(hue2rgb(p, q, h) * 255),
    Math.round(hue2rgb(p, q, h - 1/3) * 255),
  ];
}

function AppInner() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  // TODO: const [swarmVisible, setSwarmVisible] = useState(false); // Swarm viz — future
  const { config, updateConfig, theme, loading } = useConfig();

  // Show onboarding only after config loads from backend (not from defaults)
  useEffect(() => {
    if (!loading && config && !config.onboarding_completed) {
      setShowOnboarding(true);
    }
  }, [loading, config?.onboarding_completed]);

  const handleOnboardingComplete = useCallback(
    (newConfig: typeof config) => {
      setShowOnboarding(false);
      updateConfig(newConfig);
    },
    [updateConfig],
  );

  // Debug menu: Ctrl+Shift+F12 to reset onboarding (dev builds only)
  useEffect(() => {
    const handleDebug = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key === "F12") {
        e.preventDefault();
        if (confirm("Debug: Reset onboarding?")) {
          updateConfig({ ...config, onboarding_completed: false });
          setShowOnboarding(true);
        }
      }
    };
    window.addEventListener("keydown", handleDebug);
    return () => window.removeEventListener("keydown", handleDebug);
  }, [config, updateConfig]);

  const {
    state,
    createWorkspace,
    createTab,
    closeTab,
    setActiveTab,
    splitPane,
    closePane,
    setFocusedPane,
    openBrowser,
    setPanePty,
    updateSplitRatio,
    dismissToast,
    spawnClaudeTab,
  } = useMultiplexer();

  // Git info hook (side effects: updates git status in state)
  useGitInfo(state.focused_pane_id);

  // Keep ref to latest state for keyboard handler closure
  const stateRef = useRef(state);
  stateRef.current = state;

  const activeWs = useMemo(
    () => state.workspaces.find((ws) => ws.id === state.active_workspace_id),
    [state.workspaces, state.active_workspace_id],
  );

  const activeTab = useMemo(
    () => activeWs?.tabs.find((t) => t.id === activeWs.active_tab_id),
    [activeWs],
  );

  const handleNewTab = useCallback(() => {
    if (!activeWs) return;
    // Inherit cwd from focused pane
    const focusedPane = state.focused_pane_id ? state.panes[state.focused_pane_id] : null;
    const cwd = focusedPane?.cwd || undefined;
    createTab(activeWs.id, cwd);
  }, [activeWs, state.focused_pane_id, state.panes, createTab]);

  const handleCloseCurrentTab = useCallback(() => {
    if (activeWs && activeTab) {
      closeTab(activeWs.id, activeTab.id);
    }
  }, [activeWs, activeTab, closeTab]);

  const handleSplitRight = useCallback(() => {
    if (state.focused_pane_id) {
      splitPane(state.focused_pane_id, "Horizontal");
    }
  }, [state.focused_pane_id, splitPane]);

  const handleSplitDown = useCallback(() => {
    if (state.focused_pane_id) {
      splitPane(state.focused_pane_id, "Vertical");
    }
  }, [state.focused_pane_id, splitPane]);

  const handleCloseFocusedPane = useCallback(() => {
    if (state.focused_pane_id) {
      closePane(state.focused_pane_id);
    }
  }, [state.focused_pane_id, closePane]);

  const handlePtySpawned = useCallback(
    (paneId: string, ptyId: string) => {
      // Link PTY in backend + update frontend state
      invoke("mux_link_pty", { paneId, ptyId }).catch(() => {});
      setPanePty(paneId, ptyId);
    },
    [setPanePty],
  );

  const handleUpdateRatio = useCallback(
    (path: number[], ratio: number) => {
      if (activeWs && activeTab) {
        updateSplitRatio(activeWs.id, activeTab.id, path, ratio);
      }
    },
    [activeWs, activeTab, updateSplitRatio],
  );

  // Command palette commands
  const paletteCommands = useMemo(
    () =>
      buildDefaultCommands({
        newTab: handleNewTab,
        closeTab: handleCloseCurrentTab,
        splitRight: handleSplitRight,
        splitDown: handleSplitDown,
        newWorkspace: () => {
          const n = stateRef.current.workspaces.length + 1;
          createWorkspace(`Workspace ${n}`);
        },
        openSettings: () => setSettingsOpen(true),
        toggleSidebar: () => setSidebarOpen((v) => !v),
        spawnClaude: async () => {
          let folder: string | null;
          try {
            folder = await openDialog({
              directory: true,
              multiple: false,
              title: "Choose project folder for Claude",
            });
          } catch {
            return;
          }
          if (!folder) return;
          const wsId = stateRef.current.active_workspace_id ?? stateRef.current.workspaces[0]?.id;
          if (wsId) {
            let skipPerms = false;
            try {
              skipPerms = localStorage.getItem("codeterm-claude-skip-perms") === "true";
            } catch { /* ignore */ }
            spawnClaudeTab(wsId, skipPerms, folder);
          }
        },
      }),
    [
      handleNewTab,
      handleCloseCurrentTab,
      handleSplitRight,
      handleSplitDown,
      createWorkspace,
      spawnClaudeTab,
    ],
  );

  // Keyboard shortcuts — uses config keybindings + language-independent matching
  const kb = config.keybindings;
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Config-driven bindings (language-independent via e.code)
      if (matchesBinding(e, kb.new_tab)) { e.preventDefault(); handleNewTab(); return; }
      if (matchesBinding(e, kb.close_tab)) { e.preventDefault(); handleCloseCurrentTab(); return; }
      if (matchesBinding(e, kb.split_right)) { e.preventDefault(); handleSplitRight(); return; }
      if (matchesBinding(e, kb.split_down)) { e.preventDefault(); handleSplitDown(); return; }
      if (matchesBinding(e, kb.close_pane)) { e.preventDefault(); handleCloseFocusedPane(); return; }
      if (matchesBinding(e, kb.new_workspace)) {
        e.preventDefault();
        const n = stateRef.current.workspaces.length + 1;
        createWorkspace(`Workspace ${n}`);
        return;
      }
      if (matchesBinding(e, kb.copy)) {
        e.preventDefault();
        const sel = window.getSelection()?.toString();
        if (sel) navigator.clipboard.writeText(sel).catch(() => {});
        return;
      }
      if (matchesBinding(e, kb.paste)) {
        e.preventDefault();
        const focusedId = stateRef.current.focused_pane_id;
        if (focusedId) {
          const focusedPane = stateRef.current.panes[focusedId];
          if (focusedPane?.pty_id) {
            navigator.clipboard.readText().then((text) => {
              if (text) invoke("write_pty_string", { ptyId: focusedPane.pty_id!, data: text });
            }).catch(() => {});
          }
        }
        return;
      }
      if (matchesBinding(e, kb.search)) {
        e.preventDefault();
        const focusedPaneId = stateRef.current.focused_pane_id;
        if (focusedPaneId) {
          window.dispatchEvent(new CustomEvent("codeterm-toggle-search", { detail: { paneId: focusedPaneId } }));
        }
        return;
      }
      if (matchesBinding(e, kb.zoom_in)) {
        e.preventDefault();
        const newSize = Math.min(32, (config.font.size || 14) + 1);
        updateConfig({ ...config, font: { ...config.font, size: newSize } });
        return;
      }
      if (matchesBinding(e, kb.zoom_out)) {
        e.preventDefault();
        const newSize = Math.max(8, (config.font.size || 14) - 1);
        updateConfig({ ...config, font: { ...config.font, size: newSize } });
        return;
      }
      if (matchesBinding(e, kb.zoom_reset)) {
        e.preventDefault();
        updateConfig({ ...config, font: { ...config.font, size: 14 } });
        return;
      }
      if (matchesBinding(e, kb.open_settings)) {
        e.preventDefault();
        setSettingsOpen((v) => !v);
        return;
      }

      // TODO: Swarm overlay — future feature
      // if (matchesBinding(e, "Mod+Shift+G")) {
      //   e.preventDefault();
      //   setSwarmVisible((v) => !v);
      //   return;
      // }

      // Pane focus navigation (uses config bindings)
      if (matchesBinding(e, kb.focus_pane_left) || matchesBinding(e, kb.focus_pane_right) ||
          matchesBinding(e, kb.focus_pane_up) || matchesBinding(e, kb.focus_pane_down)) {
        e.preventDefault();
        const dir = matchesBinding(e, kb.focus_pane_left) ? "ArrowLeft"
          : matchesBinding(e, kb.focus_pane_right) ? "ArrowRight"
          : matchesBinding(e, kb.focus_pane_up) ? "ArrowUp" : "ArrowDown";
        const paneEls = Array.from(document.querySelectorAll("[data-pane-id]")) as HTMLElement[];
        if (paneEls.length < 2) return;
        const focusedId = stateRef.current.focused_pane_id;
        const currentEl = paneEls.find((el) => el.dataset.paneId === focusedId);
        if (!currentEl) return;
        const currentRect = currentEl.getBoundingClientRect();
        let bestEl: HTMLElement | null = null;
        let bestDist = Infinity;
        for (const el of paneEls) {
          if (el === currentEl) continue;
          const r = el.getBoundingClientRect();
          const dx = (r.left + r.width / 2) - (currentRect.left + currentRect.width / 2);
          const dy = (r.top + r.height / 2) - (currentRect.top + currentRect.height / 2);
          let valid = false;
          if (dir === "ArrowLeft" && dx < -10) valid = true;
          if (dir === "ArrowRight" && dx > 10) valid = true;
          if (dir === "ArrowUp" && dy < -10) valid = true;
          if (dir === "ArrowDown" && dy > 10) valid = true;
          if (valid) {
            const dist = Math.abs(dx) + Math.abs(dy);
            if (dist < bestDist) { bestDist = dist; bestEl = el; }
          }
        }
        if (bestEl?.dataset.paneId) setFocusedPane(bestEl.dataset.paneId);
        return;
      }

      // Tab switching: next/prev
      if (matchesBinding(e, kb.next_tab)) {
        e.preventDefault();
        if (activeWs && activeWs.tabs.length > 1) {
          const idx = activeWs.tabs.findIndex((t) => t.id === activeWs.active_tab_id);
          setActiveTab(activeWs.id, activeWs.tabs[(idx + 1) % activeWs.tabs.length].id);
        }
        return;
      }
      if (matchesBinding(e, kb.prev_tab)) {
        e.preventDefault();
        if (activeWs && activeWs.tabs.length > 1) {
          const idx = activeWs.tabs.findIndex((t) => t.id === activeWs.active_tab_id);
          setActiveTab(activeWs.id, activeWs.tabs[(idx - 1 + activeWs.tabs.length) % activeWs.tabs.length].id);
        }
        return;
      }

      // Mod+1-9: switch tab by number
      const modKey = isMac ? e.metaKey : e.ctrlKey;
      if (modKey && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
        const num = parseInt(e.code.slice(5), 10);
        if (activeWs) {
          const tab = activeWs.tabs[num - 1];
          if (tab) {
            e.preventDefault();
            setActiveTab(activeWs.id, tab.id);
          }
        }
        return;
      }

      // Ignore modifier-only key presses
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;

      // FALLBACK: If xterm.js textarea doesn't have focus, send keys directly to PTY
      const active = document.activeElement;
      const isXtermFocused = active?.classList.contains("xterm-helper-textarea");
      const isInputFocused = active?.tagName === "INPUT" || active?.tagName === "TEXTAREA" || active?.getAttribute("contenteditable") === "true";
      if (!isXtermFocused && !isInputFocused && stateRef.current.focused_pane_id) {
        const pane = stateRef.current.panes[stateRef.current.focused_pane_id];
        if (pane?.pty_id) {
          let data = "";
          if (e.ctrlKey && !e.shiftKey && e.key.length === 1) {
            const code = e.key.toUpperCase().charCodeAt(0) - 64;
            if (code > 0 && code < 32) data = String.fromCharCode(code);
          } else if (!e.ctrlKey && !e.altKey && !e.metaKey) {
            if (e.key === "Enter") data = "\r";
            else if (e.key === "Backspace") data = "\x7f";
            else if (e.key === "Tab") { data = "\t"; e.preventDefault(); }
            else if (e.key === "Escape") data = "\x1b";
            else if (e.key === "ArrowUp") data = "\x1b[A";
            else if (e.key === "ArrowDown") data = "\x1b[B";
            else if (e.key === "ArrowRight") data = "\x1b[C";
            else if (e.key === "ArrowLeft") data = "\x1b[D";
            else if (e.key === "Home") data = "\x1b[H";
            else if (e.key === "End") data = "\x1b[F";
            else if (e.key === "Delete") data = "\x1b[3~";
            else if (e.key.length === 1) data = e.key;
          }
          if (data) {
            e.preventDefault();
            invoke("write_pty_string", { ptyId: pane.pty_id, data });
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    kb,
    handleNewTab,
    handleCloseCurrentTab,
    handleSplitRight,
    handleSplitDown,
    handleCloseFocusedPane,
    openBrowser,
    createWorkspace,
    setFocusedPane,
    activeWs,
    setActiveTab,
    config,
    updateConfig,
  ]);

  // Compute CSS vars from theme
  const themeStyles = useMemo(() => themeToCSS(theme), [theme]);
  const isCRT = theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy";

  // Set color-scheme for browser defaults (scrollbars, form controls)
  useEffect(() => {
    const hex = theme.background.replace("#", "");
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    const isLight = (r + g + b) / 3 > 128;
    document.documentElement.style.colorScheme = isLight ? "light" : "dark";
  }, [theme.background]);

  return (
    <div
      className={`flex flex-col w-screen h-screen overflow-hidden ${isCRT ? "crt-mode" : ""}`}
      style={{
        ...themeStyles,
        backgroundColor: theme.background,
        color: theme.foreground,
      } as React.CSSProperties}
    >
      <Titlebar
        onToggleSidebar={() => setSidebarOpen((v) => !v)}
        onNewTab={handleNewTab}
        workspaceName={activeWs?.name}
        tabTitle={activeTab?.title}
        theme={theme}
      />

      <div className="flex flex-1 min-h-0">
        {sidebarOpen && (
          <Sidebar onOpenSettings={() => setSettingsOpen(true)} />
        )}

        <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
          {state.workspaces.length > 0 ? (
            state.workspaces.flatMap((ws) =>
              ws.tabs.map((tab) => {
                const isVisible = ws.id === state.active_workspace_id && tab.id === ws.active_tab_id;
                return (
              <div
                key={tab.id}
                className="absolute inset-0"
                style={{
                  zIndex: isVisible ? 1 : 0,
                  opacity: isVisible ? 1 : 0,
                  pointerEvents: isVisible ? "auto" : "none",
                }}
              >
                <SplitContainer
                  node={tab.split_tree}
                  panes={state.panes}
                  focusedPaneId={state.focused_pane_id}
                  onFocusPane={setFocusedPane}
                  onSplitPane={splitPane}
                  onClosePane={closePane}
                  onPtySpawned={handlePtySpawned}
                  path={[]}
                  onUpdateRatio={handleUpdateRatio}
                  theme={theme}
                  fontConfig={config.font}
                  terminalConfig={config.terminal}
                />
              </div>
                );
              })
            )
          ) : (
            <div
              className="flex items-center justify-center w-full h-full text-sm"
              style={{ color: theme.bright_black }}
            >
              {state.workspaces.length === 0
                ? "Loading..."
                : "No tabs open. Press Ctrl+Shift+T to create one."}
            </div>
          )}
        </main>
        {/* Status bar removed — git info not needed */}
        </div>
      </div>

      {/* Onboarding wizard */}
      {showOnboarding && config && (
        <Onboarding
          config={config}
          theme={theme}
          onComplete={handleOnboardingComplete}
        />
      )}

      {/* Toast notifications */}
      <NotificationToastContainer
        toasts={state.toasts}
        onDismiss={dismissToast}
      />

      {/* Settings panel */}
      {settingsOpen && (
        <SettingsPanel
          config={config}
          onUpdateConfig={updateConfig}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {/* Command palette (Cmd+K / Ctrl+K) */}
      <CommandPalette commands={paletteCommands} />

      {/* TODO: Agent swarm visualization — future feature */}
      {/* <SwarmOverlay visible={swarmVisible} onClose={() => setSwarmVisible(false)} /> */}
    </div>
  );
}

export function App() {
  return (
    <MultiplexerProvider>
      <CronProvider>
        <AppInner />
      </CronProvider>
    </MultiplexerProvider>
  );
}
