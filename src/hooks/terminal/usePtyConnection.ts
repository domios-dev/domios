import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, IS_WEB, isPtyDeadError, resetWebPtyTail, takePtyTruncation } from "../../lib/bridge";
import { subscribePaneOutput, webOutputStreamActive, type PaneOutputSink } from "../../lib/outputStream";
import { useMultiplexer } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";
import { uid } from "../../lib/uid";

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
  localFitEnabled?: boolean;
  ioEnabled?: boolean;
  resizeClientId?: string;
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
  localFitEnabled = true,
  ioEnabled = true,
  resizeClientId,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
  const { state } = useMultiplexer();
  const localFitEnabledRef = useRef(localFitEnabled);
  // Web resets the tail cursor once per mount; toggles resume it.
  const webReplayedPtysRef = useRef<Set<string>>(new Set());
  // xterm instance is keep-alive (never destroyed on tab switch); replay scrollback only once per pane lifetime.
  const replayDoneRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    localFitEnabledRef.current = localFitEnabled;
  }, [localFitEnabled]);

  const writePty = useCallback((data: string) => {
    const id = connectedPtyRef.current;
    if (id) invoke("write_pty_string", { ptyId: id, data });
  }, [connectedPtyRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || !ioEnabled) return;

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
      const predictedId = ptyId || uid();
      connectedPtyRef.current = predictedId;
      // Multiplexed SSE output stream replaces the per-pane long-poll below unless the
      // user killswitched it back via localStorage `codeterm-web-output-stream=off`.
      const useOutputStream = webOutputStreamActive();

      // Replay scrollback only on first mount — xterm is keep-alive (never destroyed on tab switch),
      // so re-sending up to 2MB of ANSI on every tab activation is wasteful.
      if (ptyId && IS_WEB) {
        if (!webReplayedPtysRef.current.has(ptyId)) {
          webReplayedPtysRef.current.add(ptyId);
          // Stream mode replays via its own pane-keyed offset tracking (outputStream.ts);
          // the long-poll tail cursor only matters to the read_pty fallback transport.
          if (!useOutputStream) resetWebPtyTail(ptyId);
        }
      } else if (ptyId) {
        if (!replayDoneRef.current.has(ptyId)) {
          replayDoneRef.current.add(ptyId);

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
        }
        // One fit() + resize on every (re)connect to sync PTY dimensions.
        requestAnimationFrame(() => {
          if (!localFitEnabledRef.current) return;
          try {
            fitAddonRef.current?.fit();
            jiggleResize(ptyId);
          } catch { /* ignore */ }
        });
      }

      // Read loop with flow control
      let reading = true;
      let firstData = true;
      let staleRespawnStarted = false;
      const WRITE_HIGH_WATERMARK = 128 * 1024;
      let pendingBytes = 0;
      let readPaused = false;
      let resumeResolve: (() => void) | null = null;
      let pendingWriteChunks: Uint8Array[] = [];
      let pendingWriteLength = 0;
      let writeFrame: number | null = null;
      const TRANSIENT_RETRY_BASE_MS = 500;
      const TRANSIENT_RETRY_MAX_MS = 8_000;
      let transientRetryMs = TRANSIENT_RETRY_BASE_MS;

      const waitForDrain = (): Promise<void> => {
        if (!readPaused) return Promise.resolve();
        return new Promise((resolve) => { resumeResolve = resolve; });
      };

      const flushPendingWrites = () => {
        writeFrame = null;
        if (!reading || disposed || pendingWriteLength === 0) return;

        let payload: Uint8Array;
        if (pendingWriteChunks.length === 1) {
          payload = pendingWriteChunks[0];
        } else {
          payload = new Uint8Array(pendingWriteLength);
          let offset = 0;
          for (const chunk of pendingWriteChunks) {
            payload.set(chunk, offset);
            offset += chunk.length;
          }
        }

        pendingWriteChunks = [];
        pendingWriteLength = 0;

        term.write(payload, () => {
          pendingBytes -= payload.length;
          if (readPaused && pendingBytes < WRITE_HIGH_WATERMARK / 2) {
            readPaused = false;
            resumeResolve?.();
            resumeResolve = null;
          }
        });

        const tail = payload.length > 200 ? payload.slice(payload.length - 200) : payload;
        try {
          const text = new TextDecoder().decode(tail);
          onOutputChunk(text);
        } catch { /* ignore decode errors */ }

        if (firstData) {
          firstData = false;
        }
      };

      const enqueueTermWrite = (chunk: Uint8Array) => {
        pendingWriteChunks.push(chunk);
        pendingWriteLength += chunk.length;
        if (writeFrame == null) {
          writeFrame = requestAnimationFrame(flushPendingWrites);
        }
      };

      // Shared by both transports: the long-poll's 404 dead-PTY error and the
      // multiplexed output stream's `{ended: true}` frame both mean the same thing.
      const respawnStalePty = async () => {
        if (staleRespawnStarted) return;
        staleRespawnStarted = true;
        reading = false;
        connectedPtyRef.current = null;
        term.writeln("\r\n\x1b[90m[PTY disappeared, reconnecting...]\x1b[0m");

        try {
          const cols = Math.max(term.cols, 80);
          const rows = Math.max(term.rows, 24);
          const paneData = state.panes[paneId];
          const cwd = consumePendingCwd(paneId) || paneData?.cwd || undefined;
          const spawnedId = await invoke<string>("spawn_pty", { cols, rows, cwd, paneId });
          if (!disposed) {
            onPtySpawned?.(paneId, spawnedId);
            connectedPtyRef.current = null;
          }
        } catch (respawnError) {
          console.warn("Failed to respawn stale PTY:", respawnError);
          term.writeln("\r\n\x1b[31m[Failed to reconnect PTY]\x1b[0m");
        }
      };

      const readLoop = async () => {
        while (reading) {
          try {
            if (readPaused) await waitForDrain();

            const data = await invoke<number[]>("read_pty", { ptyId: predictedId });
            transientRetryMs = TRANSIENT_RETRY_BASE_MS;

            const truncatedFrom = takePtyTruncation(predictedId);
            if (truncatedFrom != null) {
              term.writeln(
                `\r\n\x1b[90m[output truncated — resuming from byte ${truncatedFrom}]\x1b[0m`,
              );
            }

            if (data && data.length > 0) {
              const chunk = new Uint8Array(data);

              pendingBytes += chunk.length;
              if (pendingBytes >= WRITE_HIGH_WATERMARK) readPaused = true;

              enqueueTermWrite(chunk);
            }
          } catch (e) {
            if (!reading || disposed) break; // torn down mid-read — silent stop

            if (isPtyDeadError(e)) {
              await respawnStalePty();
              break;
            }

            // Transient transport error: retry with backoff, cursor untouched (bridge.ts
            // only advances it on a successful response), nothing printed.
            await new Promise((resolve) => setTimeout(resolve, transientRetryMs));
            transientRetryMs = Math.min(transientRetryMs * 2, TRANSIENT_RETRY_MAX_MS);
          }
        }
      };
      // Resize sync
      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        invoke("resize_pty", { ptyId: predictedId, cols, rows, clientId: resizeClientId });
      });
      cleanupFns.push(() => onResizeDisposable.dispose());

      // A daemon restart invalidates the boot-scoped byte cursor; drop it so the
      // next read_pty full-replays instead of comparing against a reset ring.
      // The output stream tracks its own boot and self-heals on mismatch (outputStream.ts).
      if (IS_WEB && !useOutputStream) {
        const onBootChanged = () => resetWebPtyTail(predictedId);
        window.addEventListener("codeterm-web-boot-changed", onBootChanged);
        cleanupFns.push(() =>
          window.removeEventListener("codeterm-web-boot-changed", onBootChanged),
        );
      }

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
          // spawn_pty links the PTY to the pane before it resolves. If this component unmounts during a chat/terminal mode switch or tab remount,
          // killing here leaves backend pane state pointing at a dead PTY. Pane/tab close paths own real teardown.
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
          if (!localFitEnabledRef.current) return;
          try {
            fitAddonRef.current?.fit();
            const actualCols = term.cols;
            const actualRows = term.rows;
            if (actualCols > 0 && actualRows > 0) {
              invoke("resize_pty", {
                ptyId: predictedId,
                cols: actualCols,
                rows: actualRows,
                clientId: resizeClientId,
              });
            }
          } catch { /* ignore */ }
        });
      }

      // Pane output arrives over the single multiplexed SSE connection (subscribePaneOutput,
      // keyed by pane id — matches the server's output_stream::sweep frame tagging) unless the
      // killswitch fell back to the per-pane long-poll (readLoop).
      let unsubscribeOutputStream: (() => void) | null = null;
      if (useOutputStream) {
        const sink: PaneOutputSink = {
          onOutput: (text) => enqueueTermWrite(new TextEncoder().encode(text)),
          onTruncated: (from) => {
            term.writeln(
              `\r\n\x1b[90m[output truncated — resuming from byte ${from}]\x1b[0m`,
            );
          },
          onEnded: () => {
            void respawnStalePty();
          },
        };
        unsubscribeOutputStream = subscribePaneOutput(paneId, sink);
      } else {
        readLoop();
      }
      cleanupFns.push(() => {
        reading = false;
        unsubscribeOutputStream?.();
        if (writeFrame != null) {
          cancelAnimationFrame(writeFrame);
          writeFrame = null;
        }
        pendingWriteChunks = [];
        pendingWriteLength = 0;
      });
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
  }, [ptyId, paneId, termReady, ioEnabled, resizeClientId, onPtySpawned]);

  return { writePty };
}
