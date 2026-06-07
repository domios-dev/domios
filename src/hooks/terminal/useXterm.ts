import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { invoke } from "../../lib/bridge";
import { mapCursorStyle, buildXtermTheme } from "../../components/Terminal/helpers";
import type { ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";
import { shouldEnableXtermA11yMode } from "./xtermTestMode";

// Desktop Playwright harness (dev-server only) OR runtime opt-in via
// /web/?a11y=1. Production builds deliberately ignore VITE_CODETERM_TEST
// so a leaked env var cannot disable the WebGL terminal renderer in release.
const E2E_TEST_MODE = shouldEnableXtermA11yMode({
  isViteDev: import.meta.env.DEV,
  viteCodeTermTest: import.meta.env.VITE_CODETERM_TEST,
  locationSearch: typeof window !== "undefined" ? window.location.search : "",
});
const MACOS_WEBGL_DISABLED = typeof navigator !== "undefined"
  && /Mac/.test(navigator.platform);

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
      // E2E-only DOM mirror of the viewport (.xterm-accessibility).
      // Enabled by the Playwright dev harness and explicit /web/?a11y=1
      // only. It degrades truecolor rendering, so normal dev/release
      // sessions keep WebGL enabled.
      screenReaderMode: E2E_TEST_MODE,
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
    if (E2E_TEST_MODE) {
      // Harness runs read terminal text from the accessibility DOM; the
      // WebGL renderer is skipped so it can't fight screenReaderMode.
      invoke("report_webgl_status", { state: "skipped_test_mode", detail: null }).catch(() => {});
    } else if (MACOS_WEBGL_DISABLED) {
      invoke("report_webgl_status", { state: "skipped_macos", detail: null }).catch(() => {});
    } else
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
