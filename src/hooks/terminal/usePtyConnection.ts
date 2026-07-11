import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, IS_WEB, isPtyDeadError, resetWebPtyTail, takePtyTruncation } from "../../lib/bridge";
import { subscribePaneOutput, webOutputStreamActive, type PaneOutputSink } from "../../lib/outputStream";
import { useMuxSelector } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";
import { uid } from "../../lib/uid";
import { createHiddenOutputBuffer } from "./hiddenOutputBuffer";
import {
  TERMINAL_WRITE_SLICE_BYTES,
  writeTerminalBytesInFrames,
  writeTerminalStringInFrames,
} from "./terminalWriteFrames";

// Trailing hidden-pane output kept for replay on show. Roughly a scrollback's
// worth: older bytes would scroll off xterm's buffer anyway, so evicting them
// (and resyncing from the backend instead) avoids a multi-second burst replay.
const HIDDEN_BUFFER_MAX_BYTES = 4 * 1024 * 1024;

async function fetchPaneHistory(paneId: string): Promise<string> {
  try {
    const raw = await invoke<string>("get_pane_history_raw", { paneId });
    if (raw) return raw;
  } catch { /* fall back to screen dump */ }
  try {
    const formatted = await invoke<string>("get_pane_text", { paneId, formatted: true });
    if (formatted) return formatted;
  } catch { /* fall back to plain */ }
  try {
    const text = await invoke<string>("get_pane_text", { paneId, formatted: false });
    if (text) return text;
  } catch { /* ignore */ }
  return "";
}

function resyncPaneHistory(term: Terminal, paneId: string): Promise<void> {
  return fetchPaneHistory(paneId).then((history) => {
    term.reset();
    if (history) writeTerminalStringInFrames(term, history, () => term.scrollToBottom());
    else term.scrollToBottom();
  });
}

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
  renderEnabled?: boolean;
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
  renderEnabled = true,
  resizeClientId,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
  const paneCwd = useMuxSelector(
    (s) => s.panes[paneId]?.cwd ?? null,
    (a, b) => a === b,
  );
  const localFitEnabledRef = useRef(localFitEnabled);
  const renderEnabledRef = useRef(renderEnabled);
  const hiddenBufRef = useRef(createHiddenOutputBuffer(HIDDEN_BUFFER_MAX_BYTES));
  const cancelVisibleReplayRef = useRef<(() => void) | null>(null);
  // Web resets the tail cursor once per mount; toggles resume it.
  const webReplayedPtysRef = useRef<Set<string>>(new Set());
  // xterm instance is keep-alive (never destroyed on tab switch); replay scrollback only once per pane lifetime.
  const replayDoneRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    localFitEnabledRef.current = localFitEnabled;
  }, [localFitEnabled]);

  useEffect(() => {
    renderEnabledRef.current = renderEnabled;
    if (!renderEnabled || hiddenBufRef.current.size === 0) return;
    const term = termRef.current;
    if (!term) return;
    const { payload, dropped } = hiddenBufRef.current.drain();
    if (dropped) {
      // Cap evicted older bytes while hidden; a truncated replay could corrupt
      // alt-screen/SGR state, so resync from the backend's bounded history.
      void resyncPaneHistory(term, paneId);
      return;
    }
    cancelVisibleReplayRef.current?.();
    cancelVisibleReplayRef.current = writeTerminalBytesInFrames(term, payload, () => {
      cancelVisibleReplayRef.current = null;
      term.scrollToBottom();
    });
  }, [renderEnabled, termRef, paneId]);

  useEffect(() => () => {
    cancelVisibleReplayRef.current?.();
    cancelVisibleReplayRef.current = null;
  }, []);

  const writePty = useCallback((data: string) => {
    const id = connectedPtyRef.current;
    if (id) invoke("write_pty_string", { ptyId: id, data });
  }, [connectedPtyRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || !ioEnabled) return;

    if (connectedPtyRef.current !== null) {
      if (ptyId && connectedPtyRef.current !== ptyId) {
      } else {
        return;
      }
    }

    let cleanupFns: (() => void)[] = [];
    let disposed = false;

    const setupAndConnect = async () => {
      const predictedId = ptyId || uid();
      connectedPtyRef.current = predictedId;
      // user killswitched it back via localStorage `codeterm-web-output-stream=off`.
      const useOutputStream = webOutputStreamActive();

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
          const history = await fetchPaneHistory(paneId);
          if (history) writeTerminalStringInFrames(term, history);
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
      let cancelActiveWrite: (() => void) | null = null;
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

        const targetLength = Math.min(pendingWriteLength, TERMINAL_WRITE_SLICE_BYTES);
        const payload = new Uint8Array(targetLength);
        let copied = 0;
        while (copied < targetLength && pendingWriteChunks.length > 0) {
          const chunk = pendingWriteChunks[0];
          const take = Math.min(chunk.length, targetLength - copied);
          payload.set(chunk.slice(0, take), copied);
          copied += take;
          if (take === chunk.length) pendingWriteChunks.shift();
          else pendingWriteChunks[0] = chunk.slice(take);
        }
        pendingWriteLength -= targetLength;

        cancelActiveWrite = writeTerminalBytesInFrames(term, payload, () => {
          cancelActiveWrite = null;
          pendingBytes -= payload.length;
          if (readPaused && pendingBytes < WRITE_HIGH_WATERMARK / 2) {
            readPaused = false;
            resumeResolve?.();
            resumeResolve = null;
          }
          if (pendingWriteLength > 0 && writeFrame == null) {
            writeFrame = requestAnimationFrame(flushPendingWrites);
          }
        });

        if (firstData) {
          firstData = false;
        }
      };

      const enqueueTermWrite = (chunk: Uint8Array) => {
        const tail = chunk.length > 200 ? chunk.slice(chunk.length - 200) : chunk;
        try {
          const text = new TextDecoder().decode(tail);
          onOutputChunk(text);
        } catch { /* ignore decode errors */ }

        if (!renderEnabledRef.current) {
          hiddenBufRef.current.push(chunk);
          if (firstData) firstData = false;
          return;
        }

        pendingWriteChunks.push(chunk);
        pendingWriteLength += chunk.length;
        if (writeFrame == null && cancelActiveWrite == null) {
          writeFrame = requestAnimationFrame(flushPendingWrites);
        }
      };

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
          const paneData = paneCwd != null ? { cwd: paneCwd } : undefined;
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

            // only advances it on a successful response), nothing printed.
            await new Promise((resolve) => setTimeout(resolve, transientRetryMs));
            transientRetryMs = Math.min(transientRetryMs * 2, TRANSIENT_RETRY_MAX_MS);
          }
        }
      };
      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        invoke("resize_pty", { ptyId: predictedId, cols, rows, clientId: resizeClientId });
      });
      cleanupFns.push(() => onResizeDisposable.dispose());

      if (IS_WEB && !useOutputStream) {
        const onBootChanged = () => resetWebPtyTail(predictedId);
        window.addEventListener("codeterm-web-boot-changed", onBootChanged);
        cleanupFns.push(() =>
          window.removeEventListener("codeterm-web-boot-changed", onBootChanged),
        );
      }

      if (!ptyId) {
        const cols = Math.max(term.cols, 80);
        const rows = Math.max(term.rows, 24);
        const cwd = consumePendingCwd(paneId) || paneCwd || undefined;
        const spawnedId = await invoke<string>("spawn_pty", {
          cols, rows, predictedId, cwd, paneId,
        });
        if (disposed) {
          // killing here leaves backend pane state pointing at a dead PTY. Pane/tab close paths own real teardown.
          return;
        }
        onPtySpawned?.(paneId, spawnedId);

        if (!cwd) {
          const defaultCwd = await invoke<string>("get_default_cwd").catch(() => null);
          if (defaultCwd) {
            invoke("mux_update_pane_cwd", { paneId, cwd: defaultCwd }).catch(() => {});
          }
        }

        onShellReady();

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
        cancelActiveWrite?.();
        cancelActiveWrite = null;
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
  }, [ptyId, paneId, paneCwd, termReady, ioEnabled, renderEnabled, resizeClientId, onPtySpawned]);

  return { writePty };
}
