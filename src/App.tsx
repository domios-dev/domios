import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Titlebar } from "./components/Titlebar/Titlebar";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { SplitContainer } from "./components/Terminal/SplitContainer";
import { NotificationToastContainer } from "./components/Notifications/NotificationToast";
import { SettingsPanel } from "./components/Settings/SettingsPanel";
import { useConfig } from "./hooks/useConfig";
import {
  MultiplexerProvider,
  useMultiplexer,
} from "./store/multiplexer";
import type { ThemeConfig } from "./lib/types";

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
    "--ct-sidebar-bg": deriveColor(theme.background, 10),
    "--ct-titlebar-bg": deriveColor(theme.background, 10),
    "--ct-border": deriveColor(theme.background, 37),
    "--ct-hover": deriveColor(theme.background, 16),
    "--ct-active": deriveColor(theme.background, 27),
    "--ct-surface": deriveColor(theme.background, 12),
    "--ct-muted": theme.bright_black,
    "--ct-muted-fg": deriveColor(theme.bright_black, 60),
    "--ct-accent": theme.blue,
    "--ct-red": theme.red,
    "--ct-green": theme.green,
    "--ct-yellow": theme.yellow,
    "--ct-orange": theme.red, // fallback for agent indicator
    // CRT glow color for Pip-Boy and similar themes
    ...(theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy" ? { "--ct-glow": theme.green + "99" } : {}),
  };
}

/**
 * Derive a color from a base by adjusting brightness.
 * Automatically lightens dark colors and darkens light ones.
 */
function deriveColor(hex: string, amount: number): string {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  const brightness = (r + g + b) / 3;
  const dir = brightness > 128 ? -1 : 1;
  const nr = Math.min(255, Math.max(0, r + amount * dir));
  const ng = Math.min(255, Math.max(0, g + amount * dir));
  const nb = Math.min(255, Math.max(0, b + amount * dir));
  return `#${nr.toString(16).padStart(2, "0")}${ng.toString(16).padStart(2, "0")}${nb.toString(16).padStart(2, "0")}`;
}

function AppInner() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { config, updateConfig, theme } = useConfig();

  const {
    state,
    createWorkspace,
    setActiveWorkspace,
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
  } = useMultiplexer();

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
    if (activeWs) {
      createTab(activeWs.id);
    }
  }, [activeWs, createTab]);

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

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+Shift shortcuts
      if (e.ctrlKey && e.shiftKey) {
        switch (e.key) {
          case "T":
          case "t":
            e.preventDefault();
            handleNewTab();
            return;
          case "W":
          case "w":
            e.preventDefault();
            handleCloseCurrentTab();
            return;
          case "D":
          case "d":
            e.preventDefault();
            handleSplitRight();
            return;
          case "E":
          case "e":
            e.preventDefault();
            handleSplitDown();
            return;
          case "Q":
          case "q":
            e.preventDefault();
            handleCloseFocusedPane();
            return;
          case "B":
          case "b":
            e.preventDefault();
            openBrowser("about:blank");
            return;
          case "N":
          case "n": {
            e.preventDefault();
            const n = stateRef.current.workspaces.length + 1;
            createWorkspace(`Workspace ${n}`);
            return;
          }
          case "C":
          case "c": {
            e.preventDefault();
            const sel = window.getSelection()?.toString();
            if (sel) {
              navigator.clipboard.writeText(sel).catch(() => {});
            }
            return;
          }
          case "V":
          case "v": {
            e.preventDefault();
            const focusedId = stateRef.current.focused_pane_id;
            if (focusedId) {
              const focusedPane = stateRef.current.panes[focusedId];
              if (focusedPane?.pty_id) {
                navigator.clipboard.readText().then((text) => {
                  if (text) {
                    invoke("write_pty_string", { ptyId: focusedPane.pty_id!, data: text });
                  }
                }).catch(() => {});
              }
            }
            return;
          }
          case "F":
          case "f": {
            e.preventDefault();
            const focusedPaneId = stateRef.current.focused_pane_id;
            if (focusedPaneId) {
              window.dispatchEvent(
                new CustomEvent("codeterm-toggle-search", {
                  detail: { paneId: focusedPaneId },
                }),
              );
            }
            return;
          }
          case "Tab": {
            // Ctrl+Shift+Tab: previous tab
            e.preventDefault();
            if (activeWs && activeWs.tabs.length > 1) {
              const idx = activeWs.tabs.findIndex((t) => t.id === activeWs.active_tab_id);
              const prev = (idx - 1 + activeWs.tabs.length) % activeWs.tabs.length;
              setActiveTab(activeWs.id, activeWs.tabs[prev].id);
            }
            return;
          }
        }
      }

      // Ctrl+Tab: next tab
      if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === "Tab") {
        e.preventDefault();
        if (activeWs && activeWs.tabs.length > 1) {
          const idx = activeWs.tabs.findIndex((t) => t.id === activeWs.active_tab_id);
          const next = (idx + 1) % activeWs.tabs.length;
          setActiveTab(activeWs.id, activeWs.tabs[next].id);
        }
        return;
      }

      // Alt+Arrow: navigate between panes
      if (e.altKey && !e.ctrlKey && !e.shiftKey) {
        if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
          e.preventDefault();
          // Find all pane elements and navigate spatially
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
            const cx = r.left + r.width / 2;
            const cy = r.top + r.height / 2;
            const dx = cx - (currentRect.left + currentRect.width / 2);
            const dy = cy - (currentRect.top + currentRect.height / 2);
            let valid = false;
            if (e.key === "ArrowLeft" && dx < -10) valid = true;
            if (e.key === "ArrowRight" && dx > 10) valid = true;
            if (e.key === "ArrowUp" && dy < -10) valid = true;
            if (e.key === "ArrowDown" && dy > 10) valid = true;
            if (valid) {
              const dist = Math.abs(dx) + Math.abs(dy);
              if (dist < bestDist) { bestDist = dist; bestEl = el; }
            }
          }
          if (bestEl?.dataset.paneId) {
            setFocusedPane(bestEl.dataset.paneId);
          }
          return;
        }
      }

      // Ctrl+, opens settings
      if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === ",") {
        e.preventDefault();
        setSettingsOpen((v) => !v);
        return;
      }

      // Ctrl+= / Ctrl+- / Ctrl+0: font zoom
      if (e.ctrlKey && !e.shiftKey && !e.altKey) {
        if (e.key === "=" || e.key === "+") {
          e.preventDefault();
          const newSize = Math.min(32, (config.font.size || 14) + 1);
          updateConfig({ ...config, font: { ...config.font, size: newSize } });
          return;
        }
        if (e.key === "-") {
          e.preventDefault();
          const newSize = Math.max(8, (config.font.size || 14) - 1);
          updateConfig({ ...config, font: { ...config.font, size: newSize } });
          return;
        }
        if (e.key === "0") {
          e.preventDefault();
          updateConfig({ ...config, font: { ...config.font, size: 14 } });
          return;
        }
      }

      // Ctrl+1-9: switch tab
      if (e.ctrlKey && !e.shiftKey && !e.altKey) {
        const num = parseInt(e.key, 10);
        if (num >= 1 && num <= 9 && activeWs) {
          const tab = activeWs.tabs[num - 1];
          if (tab) {
            e.preventDefault();
            setActiveTab(activeWs.id, tab.id);
          }
          return;
        }
      }

      // Ignore modifier-only key presses
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;

      // FALLBACK: If xterm.js textarea doesn't have focus, send keys directly to PTY
      const active = document.activeElement;
      const isXtermFocused = active?.classList.contains("xterm-helper-textarea");
      if (!isXtermFocused && stateRef.current.focused_pane_id) {
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

        <main className="flex-1 min-w-0 min-h-0 relative">
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
      </div>

      {/* Settings panel */}
      {settingsOpen && (
        <SettingsPanel
          config={config}
          onUpdateConfig={updateConfig}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}

export function App() {
  return (
    <MultiplexerProvider>
      <AppInner />
    </MultiplexerProvider>
  );
}
