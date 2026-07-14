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
import { scheduleXtermInit } from "./xtermInitScheduler";

// Playwright harness (`/?a11y=1`) can enable xterm screen-reader mode.
// Production ignores VITE_CODETERM_TEST to avoid accidental renderer disable.
const viteEnv = import.meta.env ?? {};
const E2E_TEST_MODE = shouldEnableXtermA11yMode({
  isViteDev: viteEnv.DEV,
  viteCodeTermTest: viteEnv.VITE_CODETERM_TEST,
  locationSearch: typeof window !== "undefined" ? window.location.search : "",
  locationProtocol: typeof window !== "undefined" ? window.location.protocol : "",
  locationHostname: typeof window !== "undefined" ? window.location.hostname : "",
  locationPort: typeof window !== "undefined" ? window.location.port : "",
});

export interface UseXtermOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  // Visible/focused panes init ahead of backgrounded ones in the stagger queue.
  priority?: boolean;
  renderEnabled?: boolean;
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
  priority = false,
  renderEnabled = true,
}: UseXtermOptions): UseXtermReturn {
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const initialized = useRef(false);

  const [bellFlash, setBellFlash] = useState(false);
  const [termReady, setTermReady] = useState(false);
  const [winBuildNumber, setWinBuildNumber] = useState<number>(19041);

  useEffect(() => {
    if (navigator.platform.startsWith("Win")) {
      invoke<number>("get_windows_build_number").then(setWinBuildNumber).catch(() => {});
    }
  }, []);

  // staggered one-per-frame so parallel pane mounts don't stack into one long task.
  useEffect(() => {
    if (!containerRef.current) return;
    if (initialized.current) return;
    initialized.current = true;

    let created: Terminal | null = null;
    const cancel = scheduleXtermInit(() => {
      if (!containerRef.current) {
        initialized.current = false;
        return;
      }

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
        // DOM accessibility mirror for Playwright/e2e (`/?a11y=1`).
        // It disables truecolor, so normal/dev sessions keep WebGL.
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
      if (viteEnv.DEV) {
        (containerRef.current as HTMLDivElement & { __codetermTerminal?: Terminal }).__codetermTerminal = term;
      }

      if (E2E_TEST_MODE) {
        invoke("report_webgl_status", { state: "skipped_test_mode", detail: null }).catch(() => {});
      }

      // Copy-on-select is disabled to avoid overwriting the clipboard on highlight.
      // Use Ctrl+Shift+C / Cmd+C / context menu Copy instead.

      term.onBell(() => {
        setBellFlash(true);
        setTimeout(() => setBellFlash(false), 200);
      });

      fitAddon.fit();
      term.focus();
      created = term;
      termRef.current = term;
      fitAddonRef.current = fitAddon;
      setTermReady(true);
    }, { priority });

    return () => {
      cancel();
      webglRef.current?.dispose();
      webglRef.current = null;
      created?.dispose();
      if (containerRef.current && viteEnv.DEV) {
        delete (containerRef.current as HTMLDivElement & { __codetermTerminal?: Terminal }).__codetermTerminal;
      }
      termRef.current = null;
      fitAddonRef.current = null;
      searchAddonRef.current = null;
      initialized.current = false;
      setTermReady(false);
    };
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || E2E_TEST_MODE) return;

    if (!renderEnabled || webglRef.current) return;

    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose();
        webglRef.current = null;
        invoke("report_webgl_status", { state: "context_lost", detail: null }).catch(() => {});
      });
      term.loadAddon(webgl);
      webglRef.current = webgl;
      invoke("report_webgl_status", { state: "loaded", detail: null }).catch(() => {});
    } catch (e) {
      const detail = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
      console.warn("[useXterm] WebGL renderer unavailable, falling back to DOM:", e);
      invoke("report_webgl_status", { state: "failed", detail }).catch(() => {});
    }
  }, [renderEnabled, termReady]);

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
