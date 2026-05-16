import { useEffect, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { useMultiplexer } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";

export interface UsePtyConnectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  paneId: string;
  ptyId?: string | null;
  termReady: boolean;
  onPtySpawned?: (paneId: string, ptyId: string) => void;
  onShellReady: () => void;
  onOutputChunk: (text: string) => void;
  jiggleResize: (ptyId: string) => void;
}

export interface UsePtyConnectionReturn {
  writePty: (data: string) => void;
}

export function usePtyConnection({
  termRef,
  fitAddonRef,
  connectedPtyRef,
  paneId,
  ptyId,
  termReady,
  onPtySpawned,
  onShellReady,
  onOutputChunk,
  jiggleResize,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
  const { state } = useMultiplexer();

  const writePty = useCallback((data: string) => {
    const id = connectedPtyRef.current;
    if (id) invoke("write_pty_string", { ptyId: id, data });
  }, [connectedPtyRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;

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
      const predictedId = ptyId || crypto.randomUUID();
      connectedPtyRef.current = predictedId;

      // Restore the full output history first so xterm rebuilds its
      // scrollback by interpreting the original ANSI stream. Otherwise a
      // remount (tab switch, hot reload, terminal re-init) starts xterm
      // empty and only catches live deltas — the user can't scroll past
      // whatever came in after mount. Falls back to the formatted screen
      // dump if the buffer endpoint isn't available.
      if (ptyId) {
        let restored = false;
        try {
          const raw = await invoke<string>("get_pane_history_raw", { paneId });
          if (raw) {
            term.write(raw);
            restored = true;
          }
        } catch { /* fall back to screen dump */ }

        if (!restored) {
          try {
            const formatted = await invoke<string>(
              "get_pane_text",
              { paneId, formatted: true },
            );
            if (formatted) term.write(formatted);
          } catch {
            try {
              const text = await invoke<string>(
                "get_pane_text",
                { paneId, formatted: false },
              );
              if (text) term.write(text);
            } catch { /* ignore */ }
          }
        }
        // Force TUI redraw after the restore replay so the final cursor
        // state and cursor-visible flag match the freshly-rendered screen.
        requestAnimationFrame(() => {
          try {
            fitAddonRef.current?.fit();
            if (ptyId) jiggleResize(ptyId);
          } catch { /* ignore */ }
        });
      }

      // Read loop with flow control
      let reading = true;
      let firstData = true;
      const WRITE_HIGH_WATERMARK = 128 * 1024;
      let pendingBytes = 0;
      let readPaused = false;
      let resumeResolve: (() => void) | null = null;

      const waitForDrain = (): Promise<void> => {
        if (!readPaused) return Promise.resolve();
        return new Promise((resolve) => { resumeResolve = resolve; });
      };

      const readLoop = async () => {
        while (reading) {
          try {
            if (readPaused) await waitForDrain();

            const data = await invoke<number[]>("read_pty", { ptyId: predictedId });
            if (data && data.length > 0) {
              const chunk = new Uint8Array(data);

              pendingBytes += chunk.length;
              if (pendingBytes >= WRITE_HIGH_WATERMARK) readPaused = true;

              term.write(chunk, () => {
                pendingBytes -= chunk.length;
                if (readPaused && pendingBytes < WRITE_HIGH_WATERMARK / 2) {
                  readPaused = false;
                  resumeResolve?.();
                  resumeResolve = null;
                }
              });

              // Feed output to trackers
              const tail = data.length > 200 ? data.slice(-200) : data;
              try {
                const text = new TextDecoder().decode(new Uint8Array(tail));
                onOutputChunk(text);
              } catch { /* ignore decode errors */ }

              if (firstData) {
                firstData = false;
                requestAnimationFrame(() => {
                  try { fitAddonRef.current?.fit(); } catch { /* ignore */ }
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

      // Resize sync
      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        invoke("resize_pty", { ptyId: predictedId, cols, rows });
      });
      cleanupFns.push(() => onResizeDisposable.dispose());

      // Spawn PTY if needed
      if (!ptyId) {
        const cols = Math.max(term.cols, 80);
        const rows = Math.max(term.rows, 24);
        const paneData = state.panes[paneId];
        const cwd = consumePendingCwd(paneId) || paneData?.cwd || undefined;
        const spawnedId = await invoke<string>("spawn_pty", {
          cols, rows, predictedId, cwd, paneId,
        });
        if (disposed) {
          invoke("kill_pty", { ptyId: spawnedId }).catch(() => {});
          return;
        }
        onPtySpawned?.(paneId, spawnedId);

        if (!paneData?.cwd) {
          const defaultCwd = await invoke<string>("get_default_cwd").catch(() => null);
          if (defaultCwd) {
            invoke("mux_update_pane_cwd", { paneId, cwd: defaultCwd }).catch(() => {});
          }
        }

        onShellReady();

        // Sync PTY size with actual xterm dimensions
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
      termRef.current?.writeln("\x1b[31m[Failed to connect to PTY]\x1b[0m");
    });

    return () => {
      disposed = true;
      for (const fn of cleanupFns) fn();
      connectedPtyRef.current = null;
    };
  }, [ptyId, paneId, termReady, onPtySpawned]);

  return { writePty };
}
