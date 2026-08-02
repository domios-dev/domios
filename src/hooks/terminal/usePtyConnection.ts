import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, IS_WEB, isPtyDeadError, resetWebPtyTail, takePtyTruncation } from "../../lib/bridge";
import { subscribeTabOutput, webOutputStreamActive, type TabOutputSink } from "../../lib/outputStream";
import { useMuxSelector } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";
import { uid } from "../../lib/uid";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { writeTerminalHistory } from "./terminalHistoryWrite";
import {
  captureTerminalViewport,
  restoreTerminalViewport,
  restoreTerminalViewportAfterQueue,
  type TerminalViewport,
} from "./terminalViewport";
import {
  abandonInitialTerminalHistory,
  claimInitialTerminalHistory,
  completeInitialTerminalHistory,
  type TerminalHistoryClaim,
} from "./terminalHistoryReplay";

async function fetchTabHistory(tabId: string): Promise<string> {
  try {
    const raw = await invoke<string>("get_tab_history_raw", { tabId });
    if (raw) return raw;
  } catch { /* fall back to screen dump */ }
  try {
    const formatted = await invoke<string>("get_tab_text", { tabId, formatted: true });
    if (formatted) return formatted;
  } catch { /* fall back to plain */ }
  try {
    const text = await invoke<string>("get_tab_text", { tabId, formatted: false });
    if (text) return text;
  } catch { /* ignore */ }
  return "";
}

export interface UsePtyConnectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  tabId: string;
  ptyId?: string | null;
  termReady: boolean;
  onPtySpawned?: (tabId: string, ptyId: string) => void;
  onShellReady: () => void;
  onOutputChunk: (text: string) => void;
  localFitEnabled?: boolean;
  ioEnabled?: boolean;
  renderEnabled?: boolean;
  resizeClientId?: string;
  onHistoryReplayStateChange?: (replaying: boolean) => void;
  onConnectionStateChange?: (ready: boolean) => void;
}

export interface UsePtyConnectionReturn {
  writePty: (data: string) => void;
}

export function usePtyConnection({
  termRef,
  fitAddonRef,
  connectedPtyRef,
  tabId,
  ptyId,
  termReady,
  onPtySpawned,
  onShellReady,
  onOutputChunk,
  localFitEnabled = true,
  ioEnabled = true,
  renderEnabled = true,
  resizeClientId,
  onHistoryReplayStateChange,
  onConnectionStateChange,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
  const tabCwd = useMuxSelector(
    (s) => s.tabs[tabId]?.cwd ?? null,
    (a, b) => a === b,
  );
  const tabCwdRef = useRef(tabCwd);
  const localFitEnabledRef = useRef(localFitEnabled);
  const hiddenViewportRef = useRef<TerminalViewport>({ followOutput: true, line: 0 });
  const cancelVisibleReplayRef = useRef<(() => void) | null>(null);
  const replayRequestRef = useRef(0);
  const replayTerminalRef = useRef<Terminal | null>(null);
  const viewportRestoreRequestRef = useRef(0);
  // Web resets the tail cursor once per mount; toggles resume it.
  const webReplayedPtysRef = useRef<Set<string>>(new Set());

  const cancelReplay = useCallback(() => {
    replayRequestRef.current += 1;
    cancelVisibleReplayRef.current?.();
    cancelVisibleReplayRef.current = null;
    replayTerminalRef.current?.element?.style.removeProperty("visibility");
    replayTerminalRef.current = null;
    onHistoryReplayStateChange?.(false);
  }, [onHistoryReplayStateChange]);

  const replayHistory = useCallback((
    term: Terminal,
    history: Uint8Array | string,
    viewport: TerminalViewport,
    onComplete?: () => void,
    onCancel?: () => void,
  ) => {
    cancelReplay();
    const request = replayRequestRef.current;
    replayTerminalRef.current = term;
    term.element?.style.setProperty("visibility", "hidden");
    onHistoryReplayStateChange?.(true);
    let settled = false;
    const complete = () => {
      if (request !== replayRequestRef.current) return;
      settled = true;
      onComplete?.();
      cancelVisibleReplayRef.current = null;
      restoreTerminalViewport(term, viewport);
      term.element?.style.removeProperty("visibility");
      replayTerminalRef.current = null;
      onHistoryReplayStateChange?.(false);
    };
    const cancelWrite = writeTerminalHistory(term, history, complete);
    cancelVisibleReplayRef.current = () => {
      cancelWrite();
      if (!settled) onCancel?.();
    };
  }, [cancelReplay, onHistoryReplayStateChange]);

  useEffect(() => {
    localFitEnabledRef.current = localFitEnabled;
  }, [localFitEnabled]);

  useEffect(() => {
    tabCwdRef.current = tabCwd;
  }, [tabCwd]);

  useEffect(() => {
    const term = termRef.current;
    const request = ++viewportRestoreRequestRef.current;
    if (!renderEnabled) {
      if (term) hiddenViewportRef.current = captureTerminalViewport(term);
      return;
    }
    if (term) {
      restoreTerminalViewportAfterQueue(
        term,
        hiddenViewportRef.current,
        () => request === viewportRestoreRequestRef.current,
      );
    }
  }, [renderEnabled, termRef]);

  useEffect(() => () => {
    cancelReplay();
  }, [cancelReplay]);

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
    let historyClaim: TerminalHistoryClaim | null = null;
    let historySettled = true;
    let transportStarted = false;
    const reportConnectionReady = () => {
      if (!disposed && historySettled && transportStarted) onConnectionStateChange?.(true);
    };
    onConnectionStateChange?.(false);

    const setupAndConnect = async () => {
      const predictedId = ptyId || uid();
      connectedPtyRef.current = predictedId;
      if (ptyId && resizeClientId) {
        requestAnimationFrame(() => {
          if (disposed || !localFitEnabledRef.current) return;
          try {
            fitAddonRef.current?.fit();
            if (term.cols > 0 && term.rows > 0) {
              void invoke("resize_pty", {
                ptyId: predictedId,
                cols: term.cols,
                rows: term.rows,
                clientId: resizeClientId,
              });
            }
          } catch { /* teardown */ }
        });
      }
      // user killswitched it back via localStorage `codeterm-web-output-stream=off`.
      const useOutputStream = webOutputStreamActive() || parseCompositeTabId(tabId) !== null;

      // so re-sending up to 2MB of ANSI on every tab activation is wasteful.
      if (ptyId && IS_WEB) {
        if (!webReplayedPtysRef.current.has(ptyId)) {
          webReplayedPtysRef.current.add(ptyId);
          // Stream mode replays via its own tab-keyed offset tracking (outputStream.ts);
          // the long-poll tail cursor only matters to the read_pty fallback transport.
          if (!useOutputStream) resetWebPtyTail(ptyId);
        }
      } else if (ptyId) {
        const claim = claimInitialTerminalHistory(tabId, term);
        if (claim) {
          historyClaim = claim;
          historySettled = false;
          const history = await fetchTabHistory(tabId);
          if (disposed) {
            abandonInitialTerminalHistory(claim);
            return;
          }
          if (history) {
            replayHistory(
              term,
              history,
              { followOutput: true, line: 0 },
              () => {
                completeInitialTerminalHistory(claim);
                historyClaim = null;
                historySettled = true;
                reportConnectionReady();
              },
              () => {
                abandonInitialTerminalHistory(claim);
                historyClaim = null;
                historySettled = true;
                reportConnectionReady();
              },
            );
          } else {
            completeInitialTerminalHistory(claim);
            historyClaim = null;
          }
        }
      }

      let reading = true;
      let firstData = true;
      let staleRespawnStarted = false;
      const WRITE_HIGH_WATERMARK = 128 * 1024;
      const LIVE_FLUSH_MAX_BYTES = 256 * 1024;
      let pendingBytes = 0;
      let readPaused = false;
      let resumeResolve: (() => void) | null = null;
      let pendingWriteChunks: Uint8Array[] = [];
      let pendingWriteLength = 0;
      let cancelActiveWrite: (() => void) | null = null;
      const TRANSIENT_RETRY_BASE_MS = 500;
      const TRANSIENT_RETRY_MAX_MS = 8_000;
      let transientRetryMs = TRANSIENT_RETRY_BASE_MS;

      const waitForDrain = (): Promise<void> => {
        if (!readPaused) return Promise.resolve();
        return new Promise((resolve) => { resumeResolve = resolve; });
      };

      const flushPendingWrites = () => {
        if (!reading || disposed || pendingWriteLength === 0) return;

        const targetLength = Math.min(pendingWriteLength, LIVE_FLUSH_MAX_BYTES);
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

        const completeWrite = () => {
          cancelActiveWrite = null;
          pendingBytes -= payload.length;
          if (readPaused && pendingBytes < WRITE_HIGH_WATERMARK / 2) {
            readPaused = false;
            resumeResolve?.();
            resumeResolve = null;
          }
          if (pendingWriteLength > 0) flushPendingWrites();
        };
        let cancelled = false;
        cancelActiveWrite = () => { cancelled = true; };
        term.write(payload, () => {
          if (!cancelled) completeWrite();
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

        pendingWriteChunks.push(chunk);
        pendingWriteLength += chunk.length;
        if (cancelActiveWrite == null) flushPendingWrites();
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
          const tabCwdNow = tabCwdRef.current;
          const tabData = tabCwdNow != null ? { cwd: tabCwdNow } : undefined;
          const cwd = consumePendingCwd(tabId) || tabData?.cwd || undefined;
          const spawnedId = await invoke<string>("spawn_pty", { cols, rows, cwd, tabId });
          if (!disposed) {
            onPtySpawned?.(tabId, spawnedId);
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
        const cwd = consumePendingCwd(tabId) || tabCwdRef.current || undefined;
        const spawnedId = await invoke<string>("spawn_pty", {
          cols, rows, predictedId, cwd, tabId,
        });
        if (disposed) {
          // killing here leaves backend tab state pointing at a dead PTY. Tab/tab close paths own real teardown.
          return;
        }
        onPtySpawned?.(tabId, spawnedId);

        if (!cwd) {
          const defaultCwd = await invoke<string>("get_default_cwd").catch(() => null);
          if (defaultCwd) {
            invoke("mux_update_tab_cwd", { tabId, cwd: defaultCwd }).catch(() => {});
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
        const sink: TabOutputSink = {
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
        unsubscribeOutputStream = subscribeTabOutput(tabId, sink);
      } else {
        readLoop();
      }
      transportStarted = true;
      reportConnectionReady();
      cleanupFns.push(() => {
        reading = false;
        unsubscribeOutputStream?.();
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
      onConnectionStateChange?.(false);
      if (historyClaim) abandonInitialTerminalHistory(historyClaim);
      for (const fn of cleanupFns) fn();
      connectedPtyRef.current = null;
    };
  }, [ptyId, tabId, termReady, ioEnabled, resizeClientId, onPtySpawned, replayHistory, onConnectionStateChange]);

  return { writePty };
}
