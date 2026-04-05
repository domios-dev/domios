import { useEffect, useRef, useState, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { useMultiplexer } from "../../store/multiplexer";
import { TitleLevel } from "../../lib/types";
import { consumePendingCommand } from "../../lib/pendingCommands";

export interface UseShellIntegrationOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  paneId: string;
  ptyId?: string | null;
  termReady: boolean;
}

export interface UseShellIntegrationReturn {
  shellReady: boolean;
  overlayVisible: boolean;
  feedOutput: (text: string) => void;
  markReady: () => void;
}

export function useShellIntegration({
  termRef,
  connectedPtyRef,
  paneId,
  ptyId,
  termReady,
}: UseShellIntegrationOptions): UseShellIntegrationReturn {
  const { state, setPaneTitle } = useMultiplexer();
  const stateRef = useRef(state);
  stateRef.current = state;

  const [shellReady, setShellReady] = useState(!!ptyId);
  const [overlayVisible, setOverlayVisible] = useState(!ptyId);

  // Fade out overlay after shell ready
  useEffect(() => {
    if (shellReady && overlayVisible) {
      const t = setTimeout(() => setOverlayVisible(false), 500);
      return () => clearTimeout(t);
    }
  }, [shellReady, overlayVisible]);

  // Execute pending command after shell is ready
  const pendingCommandFired = useRef(false);
  useEffect(() => {
    if (pendingCommandFired.current) return;

    const tryFire = () => {
      if (pendingCommandFired.current) return false;
      if (!shellReady) return false;
      const ptyIdNow = connectedPtyRef.current;
      if (!ptyIdNow) return false;
      const cmd = consumePendingCommand(paneId);
      if (!cmd) return true;
      pendingCommandFired.current = true;
      console.log(`[spawn] Firing pending command in pane ${paneId}: ${cmd}`);
      invoke("write_pty_string", { ptyId: ptyIdNow, data: cmd + "\r" });
      return true;
    };

    if (tryFire()) return;

    let attempts = 0;
    const interval = setInterval(() => {
      attempts++;
      if (tryFire() || attempts > 75) {
        clearInterval(interval);
        if (attempts > 75 && !pendingCommandFired.current) {
          console.warn(`[spawn] Pending command timeout for pane ${paneId}`);
        }
      }
    }, 200);
    return () => clearInterval(interval);
  }, [shellReady, paneId, connectedPtyRef]);

  // Register OSC 7 handler for CWD tracking
  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;

    const disposable = term.parser.registerOscHandler(7, (data) => {
      try {
        const url = new URL(data);
        let dir = decodeURIComponent(url.pathname);
        if (/^\/[A-Z]:/i.test(dir)) dir = dir.slice(1);
        if (dir && dir.length > 2) {
          invoke("mux_update_pane_cwd", { paneId, cwd: dir }).catch(() => {});
          const pane = stateRef.current.panes[paneId];
          if (pane && pane.title_level !== TitleLevel.User) {
            const dirName = dir.split("/").pop() || dir;
            if (dirName) setPaneTitle(paneId, dirName, TitleLevel.Auto);
          }
        }
      } catch {
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

    return () => disposable.dispose();
  }, [termReady, termRef, paneId, setPaneTitle]);

  // Feed output for shell readiness detection (Unix prompt patterns)
  const feedOutput = useCallback((text: string) => {
    if (shellReady || navigator.platform.startsWith("Win")) return;
    const clean = text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
                      .replace(/\x1b\][^\x07]*\x07/g, "")
                      .trimEnd();
    if (clean.endsWith("$") || clean.endsWith("#") || clean.endsWith("%") || clean.endsWith("\u276f")) {
      setShellReady(true);
    }
  }, [shellReady]);

  const markReady = useCallback(() => {
    setShellReady(true);
  }, []);

  return { shellReady, overlayVisible, feedOutput, markReady };
}
