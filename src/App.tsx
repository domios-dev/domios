import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "./lib/bridge";
import { IS_WEB } from "./lib/webApi";
import { Titlebar } from "./components/Titlebar/Titlebar";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { TabContent } from "./components/Terminal/TabContent";
import { NotificationToastContainer } from "./components/Notifications/NotificationToast";
import { SettingsPanel, type SectionId } from "./components/Settings/SettingsPanel";
import { MemoryPanel, type MemoryInitialState } from "./components/MemoryPanel/MemoryPanel";
import { MeshPanel } from "./components/MeshPanel/MeshPanel";
import { NavigationBackBar } from "./components/Navigation/NavigationBackBar";
import { navigationStore } from "./components/Navigation/navigationStore";
import { workerFocusStore } from "./components/MemoryPanel/workerFocusStore";
import { CommandPalette, buildDefaultCommands } from "./components/CommandPalette/CommandPalette";
import { CrashReportToast } from "./components/CrashReportToast";
import { useGitInfo } from "./hooks/useGitInfo";
import { useConfig } from "./hooks/useConfig";
import { useKeyboardInset } from "./hooks/useKeyboardInset";
import { useAgentProviders } from "./hooks/useAgentProviders";
import {
  MultiplexerProvider,
  useMultiplexer,
} from "./store/multiplexer";
import type { ThemeConfig } from "./lib/types";
import { matchesBinding, isMac } from "./lib/keybindings";
import { blendColor, deriveColor } from "./lib/colors";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { pasteToPane } from "./lib/panePasteRegistry";
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
    "--ct-blue": theme.blue,
    "--ct-magenta": theme.magenta,
    "--ct-cyan": theme.cyan,
    "--ct-bright-blue": theme.bright_blue,
    "--ct-bright-magenta": theme.bright_magenta,
    "--ct-bright-cyan": theme.bright_cyan,
    "--ct-bright-green": theme.bright_green,
    "--ct-bright-yellow": theme.bright_yellow,
    "--ct-bright-red": theme.bright_red,
    ...(theme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy" ? { "--ct-glow": theme.green + "99" } : {}),
  };
}


function AppInner() {
  useAgentProviders();
  // Phones start with the sidebar collapsed; the hamburger opens it as an
  // overlay (see the wrapper at the render site).
  const [sidebarOpen, setSidebarOpen] = useState(
    () => typeof window === "undefined" || window.innerWidth > 640,
  );
  const keyboardInset = useKeyboardInset();
  const [mainView, setMainView] = useState<"workspace" | "settings" | "memory" | "mesh">("workspace");
  const [settingsSection, setSettingsSection] = useState<SectionId | undefined>(undefined);
  const [memoryInitial, setMemoryInitial] = useState<MemoryInitialState | undefined>(undefined);
  // Lazy-mount panels: mount once on first open, stay mounted for instant switching.
  const [mountedSettings, setMountedSettings] = useState(false);
  const [mountedMemory, setMountedMemory] = useState(false);
  const [mountedMesh, setMountedMesh] = useState(false);
  const [mountedTabIds, setMountedTabIds] = useState<Set<string>>(() => new Set());
  useEffect(() => {
    if (mainView === "settings") setMountedSettings(true);
    if (mainView === "memory") setMountedMemory(true);
    if (mainView === "mesh") setMountedMesh(true);
  }, [mainView]);
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
  const setMeshOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "mesh";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "mesh" : "workspace";
    });
  }, []);
  // TODO: const [swarmVisible, setSwarmVisible] = useState(false); // Swarm viz — future
  const { config, updateConfig, theme } = useConfig();

  // Resolve usage-tracking toggle once per config change so every pane's
  // UsageBubble can read it via prop instead of subscribing to config
  // events on its own (the per-bubble `useConfig` was multiplying invokes
  // by pane count and made the webview unresponsive after many resumes).
  const usageEnabledByProvider = useMemo(() => {
    const out: Record<string, boolean> = {};
    for (const [pid, pc] of Object.entries(config.agents ?? {})) {
      out[pid] = pc?.usage_tracking_enabled === true;
    }
    return out;
  }, [config.agents]);

  const {
    state,
    createWorkspace,
    createTab,
    closeTab,
    setActiveTab,
    closePane,
    setFocusedPane,
    setPanePty,
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
  useEffect(() => {
    if (!activeTab?.id) return;
    setMountedTabIds((prev) => {
      if (prev.has(activeTab.id)) return prev;
      const next = new Set(prev);
      next.add(activeTab.id);
      return next;
    });
  }, [activeTab?.id]);
  useEffect(() => {
    const liveTabIds = new Set(state.workspaces.flatMap((ws) => ws.tabs.map((tab) => tab.id)));
    setMountedTabIds((prev) => {
      let changed = false;
      const next = new Set<string>();
      for (const id of prev) {
        if (liveTabIds.has(id)) {
          next.add(id);
        } else {
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [state.workspaces]);

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
        openSettings: () => { setSettingsSection(undefined); setSettingsOpen(true); },
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
      // App-wide paste: works when the CodeTerm window is focused regardless
      // of where the inner focus is (titlebar, sidebar, document body, an
      // unfocused pane). Matches plain Cmd+V / Ctrl+V AND the configured
      // kb.paste (default Ctrl+Shift+V).
      //
      // Skips when an actual editable element owns focus — xterm helper
      // textarea has its own attachCustomKeyEventHandler that calls
      // routeClipboardToTerm directly, and real <input>/<textarea>/
      // contenteditable elements should keep native browser paste. Both
      // cases would otherwise double-paste against this fallback.
      const plainPaste = e.code === "KeyV" && !e.shiftKey && !e.altKey
        && (isMac ? e.metaKey : e.ctrlKey);
      if (plainPaste || matchesBinding(e, kb.paste)) {
        const active = document.activeElement;
        const isXtermFocused = active?.classList.contains("xterm-helper-textarea");
        const isInputFocused = !isXtermFocused && (
          active?.tagName === "INPUT"
          || active?.tagName === "TEXTAREA"
          || active?.getAttribute("contenteditable") === "true"
        );
        if (isXtermFocused || isInputFocused) return;

        // Target the ACTIVE TAB'S pane, not the global focused_pane_id.
        // focused_pane_id can stay set on a pane from a previous tab even
        // after the user switched tabs — pasting there means the text
        // lands in an invisible terminal. The active tab's pane is what
        // the user perceives as "the selected one" they're looking at.
        const st = stateRef.current;
        const ws = st.workspaces.find((w) => w.id === st.active_workspace_id);
        const tab = ws?.tabs.find((t) => t.id === ws.active_tab_id);
        const targetId = tab?.pane_id;
        if (!targetId) return;
        if (pasteToPane(targetId)) {
          e.preventDefault();
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

  // Set color-scheme + theme vars on documentElement so portals (modals,
  // popups rendered into document.body via createPortal) inherit them.
  useEffect(() => {
    const hex = theme.background.replace("#", "");
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    const isLight = (r + g + b) / 3 > 128;
    document.documentElement.style.colorScheme = isLight ? "light" : "dark";
    for (const [k, v] of Object.entries(themeStyles)) {
      document.documentElement.style.setProperty(k, v);
    }
  }, [theme.background, themeStyles]);

  return (
    <div
      className={`flex flex-col w-screen overflow-hidden ${isCRT ? "crt-mode" : ""}`}
      style={{
        ...themeStyles,
        backgroundColor: theme.background,
        color: theme.foreground,
        // dvh tracks the REAL visible viewport on mobile (shrinks when
        // browser chrome / keyboard appear); vh does not. Desktop equal.
        // keyboardInset covers iOS Safari, where the keyboard shrinks only
        // the VISUAL viewport and dvh stays put (see useKeyboardInset).
        height: keyboardInset ? `calc(100dvh - ${keyboardInset}px)` : "100dvh",
      } as React.CSSProperties}
    >
      {/* Web mirror strips the chrome: no titlebar row — the sidebar is
          the whole shell (host picker + keep-awake live there). */}
      {!IS_WEB && (
        <Titlebar
          onToggleSidebar={() => setSidebarOpen((v) => !v)}
          onNewTab={handleNewTab}
          workspaceName={activeWs?.name}
          tabTitle={activeTab?.title}
          theme={theme}
          onOpenSettingsAbout={() => { setSettingsSection("about"); setSettingsOpen(true); }}
        />
      )}
      {IS_WEB && !sidebarOpen && (
        <button
          data-testid="web-sidebar-open"
          onClick={() => setSidebarOpen(true)}
          className="fixed top-2 left-2 z-50 flex items-center justify-center w-11 h-11 rounded-md bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] text-[var(--ct-fg)]"
          title="Open sidebar"
        >
          ☰
        </button>
      )}

      <div className="flex flex-1 min-h-0 relative">
        {sidebarOpen && (
          <>
            {/* Mobile: sidebar overlays the content with a tap-to-close
                backdrop; ≥sm it participates in the flex row as before. */}
            <div
              className="sm:hidden fixed inset-0 z-30 bg-black/40"
              onClick={() => setSidebarOpen(false)}
            />
            <div className="max-sm:fixed max-sm:inset-y-0 max-sm:left-0 max-sm:z-40 max-sm:shadow-2xl flex min-h-0">
              <Sidebar
                onOpenSettings={() => setSettingsOpen(true)}
                onOpenMemory={() => setMemoryOpen(true)}
                onOpenMesh={() => setMeshOpen(true)}
                onNavigateToTab={() => setMainView("workspace")}
                mainView={mainView}
                config={config}
              />
            </div>
          </>
        )}

        <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
          {/* All panels stay mounted — switching mainView only changes visibility.
              Unmounting the workspace would tear down xterm + re-run get_pane_text
              on return, blocking the main thread with MB of escape codes. */}
          {mountedSettings && (
            <div
              className="absolute inset-0"
              style={{
                zIndex: mainView === "settings" ? 2 : 0,
                visibility: mainView === "settings" ? "visible" : "hidden",
                pointerEvents: mainView === "settings" ? "auto" : "none",
              }}
            >
              <SettingsPanel
                config={config}
                onUpdateConfig={updateConfig}
                onClose={() => { setSettingsSection(undefined); setSettingsOpen(false); }}
                initialSection={settingsSection}
                onNavigateToTab={(tab) => {
                  setSettingsOpen(false);
                  setSettingsSection(undefined);
                  if (tab === "memory") setMemoryOpen(true);
                  else if (tab === "mesh") setMeshOpen(true);
                }}
              />
            </div>
          )}
          {mountedMemory && (
            <div
              className="absolute inset-0"
              style={{
                zIndex: mainView === "memory" ? 2 : 0,
                visibility: mainView === "memory" ? "visible" : "hidden",
                pointerEvents: mainView === "memory" ? "auto" : "none",
              }}
            >
              <MemoryPanel
                api={config.api}
                onClose={() => {
                  setMemoryOpen(false);
                  setMemoryInitial(undefined);
                }}
                initialState={memoryInitial}
                onFocusPane={handleMemoryFocusPane}
                config={config}
                onConfigUpdate={updateConfig}
              />
            </div>
          )}
          {mountedMesh && (
            <div
              className="absolute inset-0"
              style={{
                zIndex: mainView === "mesh" ? 2 : 0,
                visibility: mainView === "mesh" ? "visible" : "hidden",
                pointerEvents: mainView === "mesh" ? "auto" : "none",
              }}
            >
              <MeshPanel
                api={config.api}
                config={config}
                onConfigUpdate={updateConfig}
                onClose={() => setMeshOpen(false)}
              />
            </div>
          )}
          {state.workspaces.length > 0 ? (
            state.workspaces.flatMap((ws) =>
              ws.tabs.map((tab) => {
                const isActiveTab = ws.id === state.active_workspace_id && tab.id === ws.active_tab_id;
                const shouldMountTab = isActiveTab || mountedTabIds.has(tab.id);
                if (!shouldMountTab) return null;
                const isVisible = mainView === "workspace" && isActiveTab;
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
                <TabContent
                  pane={state.panes[tab.pane_id]}
                  isFocused={state.focused_pane_id === tab.pane_id}
                  onFocusPane={setFocusedPane}
                  onPtySpawned={handlePtySpawned}
                  theme={theme}
                  fontConfig={config.font}
                  terminalConfig={config.terminal}
                  usageEnabledByProvider={usageEnabledByProvider}
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
              Loading...
            </div>
          ) : null}
        </main>
        </div>
      </div>

      <NotificationToastContainer
        toasts={state.toasts}
        onDismiss={dismissToast}
        onNavigate={setFocusedPane}
      />

      <CommandPalette commands={paletteCommands} />

      <CrashReportToast />

      <NavigationBackBar onBack={handleBackToMemory} />
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
