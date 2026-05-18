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
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch (e) {
      console.warn("[useXterm] WebGL renderer unavailable, falling back to DOM:", e);
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

    requestAnimationFrame(() => {
      try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
    });
  }, [theme, fontConfig, terminalConfig, winBuildNumber]);

  return { termRef, fitAddonRef, searchAddonRef, bellFlash, termReady };
}
