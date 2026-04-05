import { useEffect, useRef, useCallback, useState } from "react";
import { Terminal } from "@xterm/xterm";
// WebGL disabled — causes blank screen with TUI apps like Claude Code
// import { WebglAddon } from "@xterm/addon-webgl";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useMultiplexer } from "../../store/multiplexer";
import { isMac } from "../../lib/keybindings";
import { detectAgentFromTitle } from "../../lib/agentProviders";
import { ContextMenu, type ContextMenuAction } from "./ContextMenu";
import { CompletionPopup } from "./CompletionPopup";
import { SearchBar } from "./SearchBar";
import { InputTracker } from "../../lib/completions/inputTracker";
import { CompletionEngine } from "../../lib/completions/engine";
import { defaultDefinitions } from "../../lib/completions/definitions";
import type { CompletionItem } from "../../lib/completions/types";
import { TitleLevel } from "../../lib/types";
import type { PaneStatus, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";
import { consumePendingCommand, consumePendingCwd } from "../../lib/pendingCommands";

interface TerminalPaneProps {
  paneId: string;
  ptyId?: string | null;
  isFocused?: boolean;
  hasSplits?: boolean;
  onFocus?: (paneId: string) => void;
  onPtySpawned?: (paneId: string, ptyId: string) => void;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
}

/** Map config cursor_style string to xterm CursorStyle */
function mapCursorStyle(style?: string): "block" | "underline" | "bar" {
  if (style === "underline") return "underline";
  if (style === "bar") return "bar";
  return "block";
}

/** Build xterm ITheme from ThemeConfig */
function buildXtermTheme(theme?: ThemeConfig) {
  if (!theme) {
    return {
      background: "#0d1117",
      foreground: "#c9d1d9",
      cursor: "#58a6ff",
      selectionBackground: "#264f78",
      black: "#0d1117",
      red: "#ff7b72",
      green: "#7ee787",
      yellow: "#d29922",
      blue: "#58a6ff",
      magenta: "#bc8cff",
      cyan: "#39d353",
      white: "#c9d1d9",
      brightBlack: "#484f58",
      brightRed: "#ffa198",
      brightGreen: "#56d364",
      brightYellow: "#e3b341",
      brightBlue: "#79c0ff",
      brightMagenta: "#d2a8ff",
      brightCyan: "#56d364",
      brightWhite: "#f0f6fc",
    };
  }
  return {
    background: theme.background,
    foreground: theme.foreground,
    cursor: theme.cursor,
    selectionBackground: theme.selection,
    black: theme.black,
    red: theme.red,
    green: theme.green,
    yellow: theme.yellow,
    blue: theme.blue,
    magenta: theme.magenta,
    cyan: theme.cyan,
    white: theme.white,
    brightBlack: theme.bright_black,
    brightRed: theme.bright_red,
    brightGreen: theme.bright_green,
    brightYellow: theme.bright_yellow,
    brightBlue: theme.bright_blue,
    brightMagenta: theme.bright_magenta,
    brightCyan: theme.bright_cyan,
    brightWhite: theme.bright_white,
    scrollbarSliderBackground: "rgba(128, 128, 128, 0.25)",
    scrollbarSliderHoverBackground: "rgba(128, 128, 128, 0.4)",
    scrollbarSliderActiveBackground: "rgba(128, 128, 128, 0.5)",
    overviewRulerBorder: "#00000000",
  };
}

export function TerminalPane({
  paneId,
  ptyId,
  isFocused = false,
  hasSplits = false,
  onFocus,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
}: TerminalPaneProps) {
  const termRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const connectedPtyRef = useRef<string | null>(null);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  // Search bar state
  const [searchOpen, setSearchOpen] = useState(false);

  // Visual bell state
  const [bellFlash, setBellFlash] = useState(false);

  // Windows ConPTY build number — fetched once, used for xterm windowsPty config
  const [winBuildNumber, setWinBuildNumber] = useState<number>(19041);
  useEffect(() => {
    if (navigator.platform.startsWith("Win")) {
      invoke<number>("get_windows_build_number").then(setWinBuildNumber).catch(() => {});
    }
  }, []);

  // Loading overlay — hide shell startup noise until first prompt
  const [shellReady, setShellReady] = useState(!!ptyId); // existing PTY = already ready
  const [overlayVisible, setOverlayVisible] = useState(!ptyId);

  useEffect(() => {
    if (shellReady && overlayVisible) {
      const t = setTimeout(() => setOverlayVisible(false), 500);
      return () => clearTimeout(t);
    }
  }, [shellReady, overlayVisible]);

  // Execute pending command after shell is ready — with retry for robustness
  const pendingCommandFired = useRef(false);
  useEffect(() => {
    if (pendingCommandFired.current) return;

    // Check immediately and retry every 200ms until both shell ready + PTY connected
    const tryFire = () => {
      if (pendingCommandFired.current) return false;
      if (!shellReady) return false;
      const ptyIdNow = connectedPtyRef.current;
      if (!ptyIdNow) return false;
      const cmd = consumePendingCommand(paneId);
      if (!cmd) return true; // no pending command, stop retrying
      pendingCommandFired.current = true;
      console.log(`[spawn] Firing pending command in pane ${paneId}: ${cmd}`);
      invoke("write_pty_string", { ptyId: ptyIdNow, data: cmd + "\r" });
      return true;
    };

    if (tryFire()) return;

    // Retry until fired or 15s timeout
    let attempts = 0;
    const interval = setInterval(() => {
      attempts++;
      if (tryFire() || attempts > 75) { // 75 * 200ms = 15s
        clearInterval(interval);
        if (attempts > 75 && !pendingCommandFired.current) {
          console.warn(`[spawn] Pending command timeout for pane ${paneId}`);
        }
      }
    }, 200);
    return () => clearInterval(interval);
  }, [shellReady, paneId]);

  // Completion state
  const [completionItems, setCompletionItems] = useState<CompletionItem[]>([]);
  const [completionAnchor, setCompletionAnchor] = useState({ x: 0, y: 0 });
  const trackerRef = useRef<InputTracker | null>(null);
  const engineRef = useRef<CompletionEngine | null>(null);

  // Init completion engine once
  useEffect(() => {
    const tracker = new InputTracker();
    const engine = new CompletionEngine();
    for (const def of defaultDefinitions) engine.register(def);
    trackerRef.current = tracker;
    engineRef.current = engine;
  }, []);

  const { state, updatePaneStatus, setPaneTitle, setAgentType, splitPane, closePane } = useMultiplexer();
  // Ref to access latest state in callbacks without stale closures
  const stateRef = useRef(state);
  stateRef.current = state;
  const handleClick = useCallback(() => {
    onFocus?.(paneId);
    // Focus xterm textarea, then fallback to container
    xtermRef.current?.focus();
    // Also focus the container div as fallback for keyboard capture
    termRef.current?.focus();
  }, [onFocus, paneId]);

  // --- Copy helper ---
  const copySelection = useCallback(() => {
    const selection = xtermRef.current?.getSelection();
    if (selection) {
      navigator.clipboard.writeText(selection).catch(() => {});
    }
  }, []);

  // --- Paste helper ---
  const pasteFromClipboard = useCallback(() => {
    const currentPtyId = connectedPtyRef.current;
    if (!currentPtyId) return;
    navigator.clipboard.readText().then((text) => {
      if (text) {
        invoke("write_pty_string", { ptyId: currentPtyId, data: text });
      }
    }).catch(() => {});
  }, []);

  // --- Clear terminal helper ---
  const clearTerminal = useCallback(() => {
    xtermRef.current?.clear();
  }, []);

  // --- Context menu handler ---
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  // Build context menu actions
  const isProtected = state.panes[paneId]?.protected;
  const contextMenuActions: ContextMenuAction[] = [
    { label: "Copy", shortcut: "Ctrl+Shift+C", onClick: copySelection },
    { label: "Paste", shortcut: "Ctrl+Shift+V", onClick: pasteFromClipboard },
    { label: "Clear", onClick: clearTerminal, separator: true },
    ...(!isProtected ? [
      {
        label: "Split Right",
        shortcut: "Ctrl+Shift+D",
        onClick: () => splitPane(paneId, "Horizontal"),
        separator: true,
      },
      {
        label: "Split Down",
        shortcut: "Ctrl+Shift+E",
        onClick: () => splitPane(paneId, "Vertical"),
      },
      {
        label: "Close Pane",
        shortcut: "Ctrl+Shift+Q",
        onClick: () => closePane(paneId),
        separator: true,
      },
    ] as ContextMenuAction[] : []),
  ];

  // Listen for pane-specific status events
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<{ status: PaneStatus }>(
      `pane-status-${paneId}`,
      (event) => {
        if (!cancelled) {
          updatePaneStatus(paneId, event.payload.status);
        }
      },
    ).then((fn) => {
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
  }, [paneId, updatePaneStatus]);

  // Listen for screen capture requests from API
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ pane_id: string }>("screen-capture-request", (event) => {
      if (event.payload.pane_id !== paneId) return;
      const term = xtermRef.current;
      if (!term) return;
      // Read all visible lines from terminal buffer
      const lines: string[] = [];
      const buf = term.buffer.active;
      for (let i = 0; i < buf.length; i++) {
        const line = buf.getLine(i);
        if (line) lines.push(line.translateToString(true));
      }
      // Trim trailing empty lines
      while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
      const text = lines.join("\n");
      import("@tauri-apps/api/event").then(({ emit }) => {
        emit(`screen-capture-response-${paneId}`, { text });
      });
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [paneId]);

  // Initialize xterm
  useEffect(() => {
    if (!termRef.current) return;

    const term = new Terminal({
      cursorBlink: terminalConfig?.cursor_blink ?? true,
      cursorStyle: mapCursorStyle(terminalConfig?.cursor_style),
      fontSize: fontConfig?.size ?? 14,
      fontFamily: fontConfig?.family ??
        "'JetBrains Mono', 'Cascadia Code', 'Fira Code', Consolas, monospace",
      scrollback: terminalConfig?.scrollback ?? 10000,
      theme: buildXtermTheme(theme),
      overviewRuler: { width: 4 },
      allowProposedApi: true,
      // Critical for ConPTY on Windows — enables proper raw mode + cursor key handling.
      // buildNumber must match actual OS to apply correct ConPTY workarounds.
      windowsPty: {
        backend: "conpty",
        buildNumber: winBuildNumber,
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon((_event, uri) => {
      // Open links via Tauri shell API (works in frameless window)
      invoke("open_url", { url: uri }).catch(() => {
        window.open(uri, "_blank");
      });
    }));

    // Search addon
    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);
    searchAddonRef.current = searchAddon;

    term.open(termRef.current);

    // WebGL disabled — causes rendering issues with heavy TUI apps (Claude Code)
    // Canvas renderer is more reliable for complex escape sequences
    // try {
    //   term.loadAddon(new WebglAddon());
    // } catch (e) {
    //   console.warn("WebGL addon failed, using canvas renderer", e);
    // }

    // Copy on select — auto-copy selected text to clipboard
    term.onSelectionChange(() => {
      const selection = term.getSelection();
      if (selection) {
        navigator.clipboard.writeText(selection).catch(() => {});
      }
    });

    // Visual bell — flash pane border on BEL character
    term.onBell(() => {
      setBellFlash(true);
      setTimeout(() => setBellFlash(false), 200);
    });

    // Auto-name tab from terminal title (OSC 0/2 sequences)
    // Shells set CWD, Claude Code sets task description
    term.onTitleChange((title) => {
      if (title) {
        const curPane = stateRef.current.panes[paneId];
        if (curPane && curPane.title_level === TitleLevel.User) return;
        // If an agent is running, OSC title is set by the agent (task summary) — use Agent level.
        // Otherwise it's from the shell (e.g. CWD) — use Auto level.
        const hasAgent = curPane?.agent_type != null;
        setPaneTitle(paneId, title, hasAgent ? TitleLevel.Agent : TitleLevel.Auto);
        // Detect agent from OSC title
        const detectedAgent = detectAgentFromTitle(title);
        const prevAgent = stateRef.current.panes[paneId]?.agent_type;
        // Agent exited: title no longer matches agent pattern → clear agent state
        if (!detectedAgent && prevAgent) {
          setAgentType(paneId, null);
          invoke("mux_set_pane_agent_type", { paneId, agentType: null }).catch(() => {});
        }
        if (detectedAgent) {
          setAgentType(paneId, detectedAgent);
          invoke("mux_set_pane_agent_type", { paneId, agentType: detectedAgent }).catch(() => {});
          // Reset session ID when agent is first detected (prevAgent was null).
          // This covers: fresh pane, or agent restarted after exit cleared prevAgent.
          // Don't reset on title updates while same agent runs (frequent).
          if (!prevAgent) {
            invoke("mux_set_claude_session_id", { paneId, sessionId: null }).catch(() => {});
          }
          // Detect and save the agent session ID for future resume
          // Keeps retrying because: CWD might not be set yet, JSONL might not exist yet
          const tryDetectSession = () => {
            const currentPanes = stateRef.current.panes;
            const pane = currentPanes[paneId];
            if (!pane) return; // pane gone — stop
            if (pane.agent_session_id) return; // already found — stop

            const excludeIds = Object.values(currentPanes)
              .map((p) => p.agent_session_id)
              .filter((id): id is string => !!id);
            const paneCwd = pane.cwd;
            if (!paneCwd) {
              setTimeout(tryDetectSession, 2000);
              return;
            }
            invoke<string | null>("detect_claude_session_id", { cwd: paneCwd, exclude: excludeIds })
              .then((sessionId) => {
                if (sessionId) {
                  invoke("mux_set_claude_session_id", { paneId, sessionId }).catch(() => {});
                } else {
                  setTimeout(tryDetectSession, 3000);
                }
              })
              .catch(() => {
                setTimeout(tryDetectSession, 3000);
              });
          };
          tryDetectSession();
        }
      }
    });

    // OSC 7: track working directory changes
    // Format: ESC ] 7 ; file://HOST/PATH ST
    term.parser.registerOscHandler(7, (data) => {
      try {
        const url = new URL(data);
        let dir = decodeURIComponent(url.pathname);
        // Windows: /C:/Users/... → C:/Users/...
        if (/^\/[A-Z]:/i.test(dir)) dir = dir.slice(1);
        // Only update if we got a real path (not empty)
        if (dir && dir.length > 2) {
          invoke("mux_update_pane_cwd", { paneId, cwd: dir }).catch(() => {});
          // Auto-rename tab to directory name (if not manually pinned)
          const pane = stateRef.current.panes[paneId];
          if (pane && pane.title_level !== TitleLevel.User) {
            const dirName = dir.split("/").pop() || dir;
            if (dirName) setPaneTitle(paneId, dirName, TitleLevel.Auto);
          }
        }
      } catch {
        // Some shells send just a path without file:// scheme
        if (data && data.length > 2) {
          invoke("mux_update_pane_cwd", { paneId, cwd: data }).catch(() => {});
          const pane = stateRef.current.panes[paneId];
          if (pane && pane.title_level !== TitleLevel.User) {
            const dirName = data.split("/").pop() || data;
            if (dirName) setPaneTitle(paneId, dirName, TitleLevel.Auto);
          }
        }
      }
      return true;
    });

    fitAddon.fit();
    term.focus();
    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // Observe container resize to refit terminal
    const safefit = () => {
      requestAnimationFrame(() => {
        if (fitAddonRef.current) {
          try { fitAddonRef.current.fit(); } catch { /* teardown */ }
        }
      });
    };

    const resizeObserver = new ResizeObserver(safefit);
    resizeObserver.observe(termRef.current);

    // Self-heal: refit on window focus and visibility change.
    // Handles HMR reload, tab switch, minimize/restore, chat toggle.
    const handleVisibility = () => { if (!document.hidden) safefit(); };
    const handleFocus = () => safefit();
    document.addEventListener("visibilitychange", handleVisibility);
    window.addEventListener("focus", handleFocus);

    // Periodic self-heal: if xterm dimensions drift (e.g. after HMR),
    // detect mismatch and refit. Cheap check every 2s.
    const healTimer = setInterval(() => {
      const el = termRef.current;
      if (!el || !fitAddonRef.current || !xtermRef.current) return;
      const term = xtermRef.current;
      const cellW = term.options.fontSize ?? 13;
      const expectedCols = Math.floor(el.clientWidth / (cellW * 0.6));
      if (expectedCols > 0 && Math.abs(term.cols - expectedCols) > 5) {
        safefit();
      }
    }, 2000);

    // HMR: force xterm to refit + repaint canvas after hot module replacement.
    // Without this, xterm canvas goes stale after UI code changes.
    let hmrCleanup: (() => void) | undefined;
    if (import.meta.hot) {
      const onHmr = () => {
        requestAnimationFrame(() => {
          if (!fitAddonRef.current || !xtermRef.current) return;
          const t = xtermRef.current;
          const ptyId = connectedPtyRef.current;
          try {
            fitAddonRef.current.fit();
            t.refresh(0, t.rows - 1);
            // Jiggle resize: ConPTY ignores same-size resize,
            // so shrink by 1 col then restore to force SIGWINCH → TUI redraw
            if (ptyId && t.cols > 1 && t.rows > 0) {
              const cols = t.cols, rows = t.rows;
              invoke("resize_pty", { ptyId, cols: cols - 1, rows }).then(() =>
                new Promise(r => setTimeout(r, 100))
              ).then(() => {
                invoke("resize_pty", { ptyId, cols, rows });
              });
            }
          } catch { /* ignore */ }
        });
      };
      import.meta.hot.on("vite:afterUpdate", onHmr);
      hmrCleanup = () => import.meta.hot?.off("vite:afterUpdate", onHmr);
    }

    return () => {
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("focus", handleFocus);
      clearInterval(healTimer);
      hmrCleanup?.();
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
      searchAddonRef.current = null;
    };
  }, []);

  // Hot-reload theme, font, and terminal config changes
  useEffect(() => {
    const term = xtermRef.current;
    if (!term) return;

    term.options.theme = buildXtermTheme(theme);
    term.options.fontSize = fontConfig?.size ?? 14;
    term.options.fontFamily = fontConfig?.family ??
      "'JetBrains Mono', 'Cascadia Code', 'Fira Code', Consolas, monospace";
    term.options.cursorBlink = terminalConfig?.cursor_blink ?? true;
    term.options.cursorStyle = mapCursorStyle(terminalConfig?.cursor_style);
    term.options.scrollback = terminalConfig?.scrollback ?? 10000;
    if (navigator.platform.startsWith("Win")) {
      term.options.windowsPty = { backend: "conpty", buildNumber: winBuildNumber };
    }

    // Refit after font/theme changes
    requestAnimationFrame(() => {
      try {
        fitAddonRef.current?.fit();
      } catch {
        // ignore
      }
    });
  }, [theme, fontConfig, terminalConfig, winBuildNumber]);

  // Connect to PTY — spawn if needed, always set up listeners BEFORE spawning
  useEffect(() => {
    const term = xtermRef.current;
    if (!term) return;

    // Already connected to a PTY
    if (connectedPtyRef.current !== null) {
      if (ptyId && connectedPtyRef.current !== ptyId) {
        // ptyId changed, will reconnect below
      } else {
        return;
      }
    }

    let cleanupFns: (() => void)[] = [];
    let disposed = false;

    const setupAndConnect = async () => {
      // Generate a predictable PTY ID so we can listen BEFORE spawning
      const predictedId = ptyId || crypto.randomUUID();
      connectedPtyRef.current = predictedId;

      // Restore screen state for existing PTY connections (e.g. after split remount, HMR)
      // Uses screen parser (current visible state) instead of full output history —
      // full output contains escape sequences for old terminal dimensions that cause garbled display.
      if (ptyId) {
        try {
          const screen = await invoke<string>("get_pane_screen", { paneId });
          if (screen) {
            term.write(screen);
          }
        } catch {
          // Fallback to full output if screen not available
          try {
            const output = await invoke<string>("get_pane_output", { paneId });
            if (output) {
              term.write(output);
            }
          } catch { /* ignore */ }
        }
        // Force TUI redraw: ConPTY ignores resize if size unchanged,
        // so "jiggle" — shrink by 1 col then restore — to guarantee SIGWINCH.
        requestAnimationFrame(() => {
          try {
            fitAddonRef.current?.fit();
            const cols = term.cols;
            const rows = term.rows;
            if (cols > 1 && rows > 0) {
              invoke("resize_pty", { ptyId, cols: cols - 1, rows }).then(() =>
                new Promise(r => setTimeout(r, 100))
              ).then(() => {
                invoke("resize_pty", { ptyId, cols, rows });
              });
            }
          } catch { /* ignore */ }
        });
      }

      // Poll PTY output — batch writes into single animation frame for smooth scrolling
      let reading = true;
      let firstData = true;
      let pendingChunks: Uint8Array[] = [];
      let rafScheduled = false;
      let userScrolledUp = false;

      // Track whether user has scrolled up (to preserve position during writes)
      const viewportEl = term.element?.querySelector('.xterm-viewport');
      if (viewportEl) {
        const onScroll = () => {
          const buf = term.buffer.active;
          if (buf.type === 'normal') {
            userScrolledUp = buf.viewportY < buf.baseY;
          }
        };
        viewportEl.addEventListener('scroll', onScroll);
        cleanupFns.push(() => viewportEl.removeEventListener('scroll', onScroll));
      }

      const flushToTerminal = () => {
        rafScheduled = false;
        if (pendingChunks.length === 0) return;

        // Save scroll position if user scrolled up
        const savedViewportY = userScrolledUp && term.buffer.active.type === 'normal'
          ? term.buffer.active.viewportY
          : null;

        // Merge all pending chunks into one write
        const writeData = pendingChunks.length === 1
          ? pendingChunks[0]
          : (() => {
              const total = pendingChunks.reduce((s, c) => s + c.length, 0);
              const merged = new Uint8Array(total);
              let offset = 0;
              for (const chunk of pendingChunks) {
                merged.set(chunk, offset);
                offset += chunk.length;
              }
              return merged;
            })();

        term.write(writeData, () => {
          // Restore scroll position after write completes
          if (savedViewportY !== null && userScrolledUp) {
            term.scrollToLine(savedViewportY);
          }
        });
        pendingChunks = [];
      };

      const scheduleFlush = () => {
        if (!rafScheduled) {
          rafScheduled = true;
          requestAnimationFrame(flushToTerminal);
        }
      };

      const readLoop = async () => {
        while (reading) {
          try {
            const data = await invoke<number[]>("read_pty", { ptyId: predictedId });
            if (data && data.length > 0) {
              pendingChunks.push(new Uint8Array(data));
              scheduleFlush();
              // Feed last ~200 bytes to input tracker for prompt detection
              const tail = data.length > 200 ? data.slice(-200) : data;
              try {
                const text = new TextDecoder().decode(new Uint8Array(tail));
                trackerRef.current?.feedOutput(text);
                // Unix: detect shell ready via prompt pattern (Windows uses timer after cls)
                if (!shellReady && !navigator.platform.startsWith("Win")) {
                  const clean = text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
                                    .replace(/\x1b\][^\x07]*\x07/g, "")
                                    .trimEnd();
                  if (clean.endsWith("$") || clean.endsWith("#") || clean.endsWith("%") || clean.endsWith("\u276f")) {
                    setShellReady(true);
                  }
                }
              } catch { /* ignore decode errors */ }
              if (firstData) {
                firstData = false;
                requestAnimationFrame(() => {
                  try {
                    fitAddonRef.current?.fit();
                  } catch { /* ignore */ }
                });
              }
            }
          } catch {
            if (reading) {
              term.writeln("\r\n\x1b[90m[Process exited]\x1b[0m");
            }
            break;
          }
        }
      };
      readLoop();
      cleanupFns.push(() => { reading = false; });

      // Smart Ctrl+C / Cmd+C: copy if text selected, interrupt if not
      term.attachCustomKeyEventHandler((e) => {
        if (e.type !== "keydown") return true;
        const mod = isMac ? e.metaKey : e.ctrlKey;
        if (!mod || e.shiftKey || e.altKey) return true;
        if (isMac && e.ctrlKey) return true;

        // Ctrl+C: copy if text selected, otherwise interrupt
        if (e.code === "KeyC") {
          const sel = term.getSelection();
          if (sel) {
            navigator.clipboard.writeText(sel).catch(() => {});
            term.clearSelection();
            return false;
          }
          return true;
        }

        // Cmd/Ctrl+V paste handling.
        // macOS: Cmd+V triggers a native paste event that xterm.js handles via its
        // internal textarea — text paste works automatically. We only intercept for
        // image clipboard (write path to PTY). Return true to let xterm handle text.
        // Windows: Ctrl+V does NOT trigger a native paste event, so we must handle
        // both text and images manually via clipboard API.
        if (e.code === "KeyV") {
          if (isMac) {
            // Only handle image paste — text is handled by xterm natively
            const ptyTarget = predictedId;
            navigator.clipboard.read().then(async (items) => {
              for (const item of items) {
                const imageType = item.types.find((t: string) => t.startsWith("image/"));
                if (imageType) {
                  const blob = await item.getType(imageType);
                  const ext = imageType === "image/jpeg" ? "jpg" : "png";
                  const arrayBuf = await blob.arrayBuffer();
                  const bytes = Array.from(new Uint8Array(arrayBuf));
                  const path = await invoke<string>("save_clipboard_image", { bytes, ext });
                  if (path) invoke("write_pty_string", { ptyId: ptyTarget, data: path });
                }
              }
            }).catch(() => {});
            return true; // let xterm handle the native paste for text
          } else {
            // Windows/Linux: handle everything manually.
            // preventDefault to block any native paste event that Tauri webview
            // might generate — without this, xterm's textarea receives a separate
            // 'paste' event causing double insertion.
            e.preventDefault();
            const ptyTarget = predictedId;
            navigator.clipboard.read().then(async (items) => {
              const textItem = items.find((item) => item.types.includes("text/plain"));
              if (textItem) {
                const blob = await textItem.getType("text/plain");
                const text = await blob.text();
                if (text) {
                  invoke("write_pty_string", { ptyId: ptyTarget, data: text });
                  return;
                }
              }
              for (const item of items) {
                const imageType = item.types.find((t: string) => t.startsWith("image/"));
                if (imageType) {
                  const blob = await item.getType(imageType);
                  const ext = imageType === "image/jpeg" ? "jpg" : "png";
                  const arrayBuf = await blob.arrayBuffer();
                  const bytes = Array.from(new Uint8Array(arrayBuf));
                  const path = await invoke<string>("save_clipboard_image", { bytes, ext });
                  if (path) invoke("write_pty_string", { ptyId: ptyTarget, data: path });
                  return;
                }
              }
            }).catch(() => {
              navigator.clipboard.readText().then((text) => {
                if (text) invoke("write_pty_string", { ptyId: ptyTarget, data: text });
              }).catch(() => {});
            });
            return false; // prevent xterm from processing Ctrl+V
          }
        }

        return true;
      });

      // Primary: xterm.js onData — Tab completion + PTY write
      const onDataDisposable = term.onData((data) => {
        // Ctrl+V is handled in attachCustomKeyEventHandler (user gesture context)
        if (data === "\x16") return;

        const tracker = trackerRef.current;
        const engine = engineRef.current;

        // Tab key — try custom completion
        if (data === "\t" && tracker) {
          const parsed = tracker.getCursorWord();
          if (parsed) {
            // Static definitions (claude, codeterm, etc.)
            if (engine && engine.hasDefinition(parsed.command)) {
              const items = engine.getCompletions(parsed);
              if (items.length === 1) {
                const eraseCount = parsed.currentArg.length;
                const erase = "\x7f".repeat(eraseCount);
                const insert = items[0].text + " ";
                invoke("write_pty_string", { ptyId: predictedId, data: erase + insert });
                tracker.feed(erase + insert);
                return;
              }
              if (items.length > 1) {
                const rect = termRef.current?.getBoundingClientRect();
                setCompletionItems(items);
                setCompletionAnchor({
                  x: (rect?.left ?? 0) + 100,
                  y: (rect?.top ?? 0) + (term.buffer.active.cursorY + 1) * 18,
                });
                return;
              }
            }
          }
        }

        // Dismiss completion popup on any non-Tab input
        if (data !== "\t") {
          setCompletionItems([]);
        }

        // Track input + forward to PTY
        tracker?.feed(data);
        invoke("write_pty_string", { ptyId: predictedId, data });
      });
      cleanupFns.push(() => onDataDisposable.dispose());

      // Fallback: capture keyboard at container level for when xterm textarea loses focus.
      // Check both className and tagName to reliably detect xterm's internal textarea.
      const containerEl = termRef.current;
      if (containerEl) {
        const handleKeyDown = (e: KeyboardEvent) => {
          // Don't handle if xterm textarea is focused (onData handles it)
          const active = document.activeElement;
          if (
            active instanceof HTMLTextAreaElement &&
            active.closest(".xterm")
          ) return;

          // Don't capture modifier-only keys or shortcuts
          if (e.ctrlKey && e.shiftKey) return;
          if (e.altKey) return;
          if (e.key === "Shift" || e.key === "Control" || e.key === "Alt" || e.key === "Meta") return;

          e.preventDefault();
          e.stopPropagation();

          let data = "";
          if (e.ctrlKey) {
            // Ctrl+letter = control code
            const code = e.key.toUpperCase().charCodeAt(0) - 64;
            if (code > 0 && code < 32) data = String.fromCharCode(code);
          } else if (e.key === "Enter") {
            data = "\r";
          } else if (e.key === "Backspace") {
            data = "\x7f";
          } else if (e.key === "Tab") {
            data = "\t";
          } else if (e.key === "Escape") {
            data = "\x1b";
          } else if (e.key === "ArrowUp") {
            data = "\x1b[A";
          } else if (e.key === "ArrowDown") {
            data = "\x1b[B";
          } else if (e.key === "ArrowRight") {
            data = "\x1b[C";
          } else if (e.key === "ArrowLeft") {
            data = "\x1b[D";
          } else if (e.key.length === 1) {
            data = e.key;
          }

          if (data) {
            invoke("write_pty_string", { ptyId: predictedId, data });
          }
        };
        containerEl.addEventListener("keydown", handleKeyDown, true);
        cleanupFns.push(() => containerEl.removeEventListener("keydown", handleKeyDown, true));

        // Make container focusable
        containerEl.tabIndex = 0;
      }

      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        invoke("resize_pty", { ptyId: predictedId, cols, rows });
      });
      cleanupFns.push(() => onResizeDisposable.dispose());

      // 2. NOW spawn PTY if needed (listeners are ready to catch output)
      if (!ptyId) {
        // Ensure minimum size (hidden tabs may report 0)
        const cols = Math.max(term.cols, 80);
        const rows = Math.max(term.rows, 24);
        // Pass saved CWD from pane data (session restore)
        const paneData = state.panes[paneId];
        const cwd = consumePendingCwd(paneId) || paneData?.cwd || undefined;
        const spawnedId = await invoke<string>("spawn_pty", {
          cols, rows, predictedId, cwd, paneId,
        });
        if (disposed) {
          invoke("kill_pty", { ptyId: spawnedId }).catch(() => {});
          return;
        }
        // Link PTY to pane in backend
        await invoke("mux_link_pty", { paneId, ptyId: spawnedId });
        onPtySpawned?.(paneId, spawnedId);

        // Save initial CWD on the pane (if not already set from session restore)
        if (!paneData?.cwd) {
          // Default CWD is the app's working directory
          const defaultCwd = await invoke<string>("get_default_cwd").catch(() => null);
          if (defaultCwd) {
            invoke("mux_update_pane_cwd", { paneId, cwd: defaultCwd }).catch(() => {});
          }
        }

        // OSC 7 CWD tracking is handled invisibly at PTY spawn time
        // (shell_integration.rs: ZDOTDIR for zsh, PROMPT_COMMAND for bash,
        // -NoExit -Command for PowerShell on Windows). No visible injection needed.
        setShellReady(true);

        // Session resume is handled by backend (spawn_pty checks agent_session_id
        // and sends `claude --resume` after shell is ready). Works for all panes
        // including non-visible tabs that don't have TerminalPane mounted.

        // Sync PTY size with actual xterm dimensions after connection
        requestAnimationFrame(() => {
          try {
            fitAddonRef.current?.fit();
            const actualCols = term.cols;
            const actualRows = term.rows;
            if (actualCols > 0 && actualRows > 0) {
              invoke("resize_pty", { ptyId: predictedId, cols: actualCols, rows: actualRows });
            }
          } catch { /* ignore */ }
        });
      }
    };

    setupAndConnect().catch((e) => {
      console.warn("Failed to setup PTY:", e);
      term.writeln("\x1b[31m[Failed to connect to PTY]\x1b[0m");
    });

    return () => {
      disposed = true;
      for (const fn of cleanupFns) {
        fn();
      }
      connectedPtyRef.current = null;
    };
  }, [ptyId, paneId, onPtySpawned]);

  // Refit + focus xterm when pane becomes focused
  useEffect(() => {
    if (isFocused) {
      requestAnimationFrame(() => {
        try {
          fitAddonRef.current?.fit();
          xtermRef.current?.focus();
        } catch {
          // ignore
        }
      });
    }
  }, [isFocused]);

  // Expose searchOpen toggle for keyboard shortcut from App
  useEffect(() => {
    const el = termRef.current?.closest("[data-pane-id]");
    if (el) {
      (el as HTMLElement).dataset.paneId = paneId;
    }
  }, [paneId]);

  // Listen for search toggle event on this pane
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.paneId === paneId) {
        setSearchOpen((v) => !v);
      }
    };
    window.addEventListener("codeterm-toggle-search", handler);
    return () => window.removeEventListener("codeterm-toggle-search", handler);
  }, [paneId]);

  // Listen for pane resize events (triggered after divider drag ends)
  useEffect(() => {
    const handler = () => {
      requestAnimationFrame(() => {
        try {
          fitAddonRef.current?.fit();
          // Also sync PTY size after refit
          const term = xtermRef.current;
          const currentPtyId = connectedPtyRef.current;
          if (term && currentPtyId && term.cols > 0 && term.rows > 0) {
            invoke("resize_pty", { ptyId: currentPtyId, cols: term.cols, rows: term.rows });
          }
        } catch {
          // ignore fit errors
        }
      });
    };
    window.addEventListener("codeterm-pane-resize", handler);
    return () => window.removeEventListener("codeterm-pane-resize", handler);
  }, []);

  const topAccent = bellFlash
    ? "var(--ct-yellow, #d29922)"
    : hasSplits && isFocused
      ? "var(--ct-muted-fg, #8b949e)"
      : "transparent";

  return (
    <div
      className="w-full h-full relative"
      data-pane-id={paneId}
      style={{
        boxSizing: "border-box",
        borderTop: hasSplits ? `1px solid ${topAccent}` : "none",
        transition: bellFlash ? "border-color 0.1s ease-in" : "border-color 0.15s ease-out",
      }}
      onMouseDown={handleClick}
      onContextMenu={handleContextMenu}
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onDrop={async (e) => {
        e.preventDefault();
        e.stopPropagation();
        const ptyId = connectedPtyRef.current;
        if (!ptyId) return;

        const files = Array.from(e.dataTransfer.files || []);
        if (files.length === 0) {
          const text = e.dataTransfer.getData("text/plain");
          if (text) {
            // Text drop — copy to clipboard + paste via \x16
            await navigator.clipboard.writeText(text);
            invoke("write_pty_string", { ptyId, data: "\x16" });
          }
          return;
        }

        for (const file of files) {
          const name = file.name.toLowerCase();
          const isImage = /\.(png|jpg|jpeg|gif|webp|bmp|svg)$/.test(name);

          if (isImage) {
            // Image: save to temp → write path directly (don't touch clipboard!)
            try {
              const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
              const ext = name.split(".").pop() || "png";
              const path = await invoke<string>("save_clipboard_image", { bytes, ext });
              await invoke("write_pty_string", { ptyId, data: path });
              // Delay between files so Claude processes each one
              if (files.length > 1) await new Promise(r => setTimeout(r, 300));
            } catch (err) {
              console.error("[drop] Image error:", err);
            }
          } else {
            // Text files: read content and paste via bracketed paste mode
            // Binary/large files: paste path
            const filePath = (file as any).path || file.name;
            const isText = /\.(txt|md|ts|tsx|js|jsx|py|rs|go|java|c|cpp|h|hpp|json|toml|yaml|yml|html|css|scss|sh|bat|ps1|csv|xml|sql|log|env|conf|cfg|ini|rb|php|swift|kt|dart|lua|r|m|pl|ex|exs|hs|clj|scala)$/i.test(name);
            if (isText) {
              try {
                const text = await file.text();
                if (text.length < 100000) {
                  // Bracketed paste: \x1b[200~ ... \x1b[201~ — Claude Code shows as pasted block
                  const bracketedPaste = `\x1b[200~${text}\x1b[201~`;
                  await invoke("write_pty_string", { ptyId, data: bracketedPaste });
                } else {
                  await invoke("write_pty_string", { ptyId, data: `@${filePath}` });
                }
              } catch {
                await invoke("write_pty_string", { ptyId, data: filePath });
              }
            } else {
              // Binary/unknown files: paste path
              await invoke("write_pty_string", { ptyId, data: filePath });
            }
            if (files.length > 1) await new Promise(r => setTimeout(r, 200));
          }
        }
      }}
    >
      {/* Search bar */}
      {searchOpen && (
        <SearchBar
          searchAddon={searchAddonRef.current}
          onClose={() => {
            setSearchOpen(false);
            xtermRef.current?.focus();
          }}
        />
      )}

      <div
        ref={termRef}
        className="w-full h-full"
        style={{ padding: 8, opacity: overlayVisible ? 0 : 1, transition: "opacity 0.3s ease-in" }}
        onClick={() => xtermRef.current?.focus()}
      />

      {/* Loading overlay — hides shell startup noise with premium breathing animation */}
      {overlayVisible && (
        <div
          className={`absolute inset-0 ${!shellReady ? "terminal-loading-overlay" : ""}`}
          style={{
            background: "var(--ct-bg, #0d1117)",
            opacity: shellReady ? 0 : 1,
            transition: "opacity 0.3s ease-out",
          }}
        >
          {/* Breathing bar at bottom center */}
          {!shellReady && (
            <div className="absolute bottom-4 left-1/2 -translate-x-1/2">
              <div className="terminal-breathing-bar" />
            </div>
          )}
        </div>
      )}

      {/* Context menu */}
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          actions={contextMenuActions}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Tab completion popup */}
      {completionItems.length > 0 && (
        <CompletionPopup
          items={completionItems}
          anchorX={completionAnchor.x}
          anchorY={completionAnchor.y}
          onAccept={(item) => {
            const tracker = trackerRef.current;
            const parsed = tracker?.getCursorWord();
            const eraseCount = parsed?.currentArg.length ?? 0;
            const erase = "\x7f".repeat(eraseCount);
            const insert = item.text + " ";
            const ptyId = connectedPtyRef.current;
            if (ptyId) invoke("write_pty_string", { ptyId, data: erase + insert });
            tracker?.feed(erase + insert);
            setCompletionItems([]);
          }}
          onDismiss={() => setCompletionItems([])}
        />
      )}
    </div>
  );
}
