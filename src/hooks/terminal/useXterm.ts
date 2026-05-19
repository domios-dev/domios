import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { invoke } from "@tauri-apps/api/core";
import { mapCursorStyle, buildXtermTheme } from "../../components/Terminal/helpers";
import type { ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";

export interface UseXtermOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  /**
   * Pane's agent_type. When set, the host cursor goes idle (no blink,
   * no inactive draw) and is recoloured to match the terminal
   * background so it becomes effectively invisible. TUI agents
   * (claude/codex/opencode/cursor) render their own input cursor
   * inside their input widget — the host xterm cursor on top of that
   * just produces visible flicker as the TUI reposition-loop runs
   * (see openai/codex#9081, #11063 — the codex side has elected not
   * to fix this on their end).
   */
  agentType?: string | null;
}

export interface UseXtermReturn {
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  searchAddonRef: React.MutableRefObject<SearchAddon | null>;
  bellFlash: boolean;
  termReady: boolean;
}

export function useXterm({
  containerRef,
  theme,
  fontConfig,
  terminalConfig,
  agentType,
}: UseXtermOptions): UseXtermReturn {
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const initialized = useRef(false);

  const [bellFlash, setBellFlash] = useState(false);
  const [termReady, setTermReady] = useState(false);
  const [winBuildNumber, setWinBuildNumber] = useState<number>(19041);

  // Fetch Windows build number once
  useEffect(() => {
    if (navigator.platform.startsWith("Win")) {
      invoke<number>("get_windows_build_number").then(setWinBuildNumber).catch(() => {});
    }
  }, []);

  // Initialize xterm
  useEffect(() => {
    if (!containerRef.current) return;
    if (initialized.current) return;
    initialized.current = true;

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
      windowsPty: {
        backend: "conpty",
        buildNumber: winBuildNumber,
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon((_event, uri) => {
      invoke("open_url", { url: uri }).catch(() => {
        window.open(uri, "_blank");
      });
    }));

    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);
    searchAddonRef.current = searchAddon;

    const unicode11 = new Unicode11Addon();
    term.loadAddon(unicode11);
    term.unicode.activeVersion = "11";

    term.open(containerRef.current);

    // WebGL renderer — load AFTER term.open() so the canvas exists.
    // The default DOM renderer drops subtle TUI background colors used
    // by Codex / Opencode TUIs (gray-tinted input + message blocks).
    // WebGL renders 24-bit truecolor backgrounds per-cell correctly
    // and is 10x faster on top of it.
    //
    // If WebGL context creation fails (some Linux GPUs, headless),
    // fall back silently to the DOM renderer — xterm.js's default.
    // We push the success/failure into the daemon log via
    // `report_webgl_status` so the rendering health is observable
    // without opening browser devtools.
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose();
        invoke("report_webgl_status", { state: "context_lost", detail: null }).catch(() => {});
      });
      term.loadAddon(webgl);
      invoke("report_webgl_status", { state: "loaded", detail: null }).catch(() => {});
    } catch (e) {
      const detail = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
      console.warn("[useXterm] WebGL renderer unavailable, falling back to DOM:", e);
      invoke("report_webgl_status", { state: "failed", detail }).catch(() => {});
    }

    // Copy-on-select removed: silently writing the clipboard every time
    // the user highlights text is surprising and clobbers whatever they
    // had copied moments earlier. Ctrl+Shift+C / Cmd+C with a selection
    // and the right-click "Copy" item both still work.

    // Visual bell
    term.onBell(() => {
      setBellFlash(true);
      setTimeout(() => setBellFlash(false), 200);
    });

    fitAddon.fit();
    term.focus();
    termRef.current = term;
    fitAddonRef.current = fitAddon;
    setTermReady(true);
    // Apply agent-type cursor masking now in case the prop is already
    // set on first mount (re-mount after pane restore). The reactive
    // useEffect below handles subsequent changes.
    applyAgentCursorMasking(term, agentType, theme);

    return () => {
      term.dispose();
      termRef.current = null;
      fitAddonRef.current = null;
      searchAddonRef.current = null;
      initialized.current = false;
      setTermReady(false);
    };
  }, []);

  // Hot-reload theme, font, terminal config
  useEffect(() => {
    const term = termRef.current;
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
    // Re-apply agent masking after theme reload (theme contains
    // cursor/background which the masking overrides).
    applyAgentCursorMasking(term, agentType, theme);

    requestAnimationFrame(() => {
      try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
    });
  }, [theme, fontConfig, terminalConfig, winBuildNumber, agentType]);

  return { termRef, fitAddonRef, searchAddonRef, bellFlash, termReady };
}

/**
 * When a pane runs an interactive TUI agent (codex/claude/opencode/
 * cursor), the agent draws its own input cursor inside its input
 * widget. xterm's host cursor on top of that just flickers as the
 * TUI's redraw loop repositions the cursor between status rows and
 * the input area (codex specifically: openai/codex#9081 — closed
 * "not planned" — and #11063). Mask the host cursor by killing the
 * blink animation and recolouring it to match the terminal
 * background; restore on agent removal.
 */
function applyAgentCursorMasking(
  term: Terminal,
  agentType: string | null | undefined,
  theme: ThemeConfig | undefined,
) {
  const base = buildXtermTheme(theme);
  if (agentType && agentType.length > 0) {
    term.options.cursorBlink = false;
    term.options.cursorInactiveStyle = "none";
    term.options.theme = { ...base, cursor: base.background, cursorAccent: base.background };
  } else {
    term.options.theme = base;
  }
}
