import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Titlebar } from "./components/Titlebar/Titlebar";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { SplitContainer } from "./components/Terminal/SplitContainer";
import { NotificationToastContainer } from "./components/Notifications/NotificationToast";
import { SettingsPanel } from "./components/Settings/SettingsPanel";
import { CronPanel } from "./components/CronPanel/CronPanel";
import { Onboarding } from "./components/Onboarding/Onboarding";
import { MemoryPanel, type MemoryInitialState } from "./components/MemoryPanel/MemoryPanel";
import { NavigationBackBar } from "./components/Navigation/NavigationBackBar";
import { navigationStore } from "./components/Navigation/navigationStore";
import { workerFocusStore } from "./components/MemoryPanel/workerFocusStore";
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
import { blendColor, deriveColor } from "./lib/colors";
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


function AppInner() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [mainView, setMainView] = useState<"workspace" | "settings" | "memory" | "cron">("workspace");
  const [memoryInitial, setMemoryInitial] = useState<MemoryInitialState | undefined>(undefined);
  const [cronInitialId, setCronInitialId] = useState<string | undefined>(undefined);
  // Tracks which cron is currently shown in the panel's detail view (or
  // undefined when the panel is on the dashboard). Synced from CronPanel
  // via onActiveCronChange so the sidebar can highlight the matching row.
  const [activeCronId, setActiveCronId] = useState<string | undefined>(undefined);
  const setSettingsOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "settings";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "settings" : "workspace";
    });
  }, []);
  const setMemoryOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "memory";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "memory" : "workspace";
    });
  }, []);
  const openCronPanel = useCallback((jobId?: string) => {
    setCronInitialId(jobId);
    setActiveCronId(jobId);
    setMainView("cron");
  }, []);
  const closeCronPanel = useCallback(() => {
    setCronInitialId(undefined);
    setActiveCronId(undefined);
    setMainView("workspace");
  }, []);
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

  // When user focuses any pane (via sidebar click, Ctrl+Tab, etc), return to workspace view
  const lastFocusedRef = useRef(state.focused_pane_id);
  useEffect(() => {
    if (state.focused_pane_id !== lastFocusedRef.current) {
      lastFocusedRef.current = state.focused_pane_id;
      if (state.focused_pane_id != null && mainView !== "workspace") {
        setMainView("workspace");
      }
    }
  }, [state.focused_pane_id, mainView]);

  // Keep ref to latest state for keyboard handler closure
  const stateRef = useRef(state);
  stateRef.current = state;

  const handleBackToMemory = useCallback(() => {
    const ret = navigationStore.get();
    const focused = stateRef.current.focused_pane_id;
    if (focused) workerFocusStore.unfocus(focused);
    if (ret) {
      setMemoryInitial({
        selectedId: ret.memoryState.selectedNodeId,
        feedVisible: ret.memoryState.feedVisible,
        workerPanelVisible: ret.memoryState.workerPanelVisible,
      });
    }
    setMemoryOpen(true);
    navigationStore.clear();
  }, []);

  const handleMemoryFocusPane = useCallback((paneId: string) => {
    setFocusedPane(paneId);
  }, []);

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
        newWorkspace: () => {
          openDialog({ directory: true, multiple: false, title: "Choose workspace folder" })
            .then((folder) => {
              if (!folder) return;
              const basename = folder.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
              createWorkspace(basename, folder);
            })
            .catch(() => {});
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
      if (matchesBinding(e, kb.close_pane)) { e.preventDefault(); handleCloseFocusedPane(); return; }
      if (matchesBinding(e, kb.new_workspace)) {
        e.preventDefault();
        openDialog({ directory: true, multiple: false, title: "Choose workspace folder" })
          .then((folder) => {
            if (!folder) return;
            const basename = folder.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
            createWorkspace(basename, folder);
          })
          .catch(() => {});
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

      // Memory panel: Cmd+Shift+M / Ctrl+Shift+M
      if (e.code === "KeyM" && e.shiftKey && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setMemoryOpen((v) => !v);
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
          <Sidebar
            onOpenSettings={() => setSettingsOpen(true)}
            onOpenMemory={() => setMemoryOpen(true)}
            onOpenCron={openCronPanel}
            onNavigateToTab={() => setMainView("workspace")}
            mainView={mainView}
            activeCronId={activeCronId}
          />
        )}

        <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
          {mainView === "settings" && (
            <SettingsPanel
              config={config}
              onUpdateConfig={updateConfig}
              onClose={() => setSettingsOpen(false)}
            />
          )}
          {mainView === "memory" && (
            <MemoryPanel
              api={config.api}
              onClose={() => {
                setMemoryOpen(false);
                setMemoryInitial(undefined);
              }}
              initialState={memoryInitial}
              onFocusPane={handleMemoryFocusPane}
            />
          )}
          {mainView === "cron" && (
            <CronPanel
              initialView={
                cronInitialId
                  ? { type: "detail", cronId: cronInitialId }
                  : { type: "dashboard" }
              }
              onClose={closeCronPanel}
              onActiveCronChange={setActiveCronId}
            />
          )}
          {mainView === "workspace" && state.workspaces.length > 0 ? (
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
          ) : mainView === "workspace" ? (
            <div
              className="flex items-center justify-center w-full h-full text-sm"
              style={{ color: theme.bright_black }}
            >
              {state.workspaces.length === 0
                ? "Loading..."
                : "No tabs open. Press Ctrl+Shift+T to create one."}
            </div>
          ) : null}
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
        onNavigate={setFocusedPane}
      />

      {/* Command palette (Cmd+K / Ctrl+K) */}
      <CommandPalette commands={paletteCommands} />

      {/* Navigation back bar (visible only when navigationStore has a return target) */}
      <NavigationBackBar onBack={handleBackToMemory} />

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
