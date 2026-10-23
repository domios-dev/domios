import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "../../lib/bridge";
import { subscribeTabOutput, type TabOutputSink } from "../../lib/outputStream";
import { useMuxSelector } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";
import { uid } from "../../lib/uid";
import { isPlanLockedRefusal } from "../../lib/spawnErrorMessage";
import { shouldAttachTerminalOutput, writeTerminalOutputAndWait } from "./terminalOutputWrite";
import { writeTerminalInput } from "./terminalInputWrite";
import {
  captureTerminalViewport,
  restoreTerminalViewportAfterQueue,
  type TerminalViewport,
} from "./terminalViewport";
import { setTerminalReadingHistory } from "./terminalHistoryReaders";

export function shouldFetchInitialTerminalHistory(
  _ptyId: string | null | undefined,
  _isWeb: boolean,
  _isRemote: boolean,
): boolean {
  return false;
}

export interface UsePtyConnectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  tabId: string;
  ptyId?: string | null;
  termReady: boolean;
  onPtySpawned?: (tabId: string, ptyId: string) => void;
  onShellReady: () => void;
  onOutputChunk: (text: string) => void;
  ioEnabled?: boolean;
  renderEnabled?: boolean;
  onHistoryReplayStateChange?: (replaying: boolean) => void;
  onFrameReady?: (ptyId: string) => void;
  onConnectionStateChange?: (ready: boolean) => void;
}

export interface UsePtyConnectionReturn {
  writePty: (data: string) => void;
}

export function usePtyConnection({
  termRef,
  connectedPtyRef,
  tabId,
  ptyId,
  termReady,
  onPtySpawned,
  onShellReady,
  onOutputChunk,
  ioEnabled = true,
  renderEnabled = true,
  onHistoryReplayStateChange,
  onFrameReady,
  onConnectionStateChange,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
  const currentCallbacks = {
    onPtySpawned, onShellReady, onOutputChunk,
    onHistoryReplayStateChange, onFrameReady, onConnectionStateChange,
  };
  const callbacks = useRef(currentCallbacks);
  callbacks.current = currentCallbacks;
  const tabCwd = useMuxSelector(
    (s) => s.tabs[tabId]?.cwd ?? null,
    (a, b) => a === b,
  );
  const tabCwdRef = useRef(tabCwd);
  const hiddenViewportRef = useRef<TerminalViewport>({ followOutput: true, line: 0 });
  const viewportRestoreRequestRef = useRef(0);

  useEffect(() => {
    tabCwdRef.current = tabCwd;
  }, [tabCwd]);

  useEffect(() => {
    const term = termRef.current;
    const request = ++viewportRestoreRequestRef.current;
    if (!renderEnabled) {
      if (term) {
        hiddenViewportRef.current = captureTerminalViewport(term);
        setTerminalReadingHistory(tabId, !hiddenViewportRef.current.followOutput);
      }
      return;
    }
    setTerminalReadingHistory(tabId, false);
    if (term) {
      restoreTerminalViewportAfterQueue(
        term,
        hiddenViewportRef.current,
        () => request === viewportRestoreRequestRef.current,
      );
    }
  }, [renderEnabled, tabId, termRef]);

  useEffect(() => () => setTerminalReadingHistory(tabId, false), [tabId]);

  const writePty = useCallback((data: string) => {
    const id = connectedPtyRef.current;
    if (id) void writeTerminalInput(tabId, id, data);
  }, [tabId, connectedPtyRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || !shouldAttachTerminalOutput(ioEnabled, renderEnabled)) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    callbacks.current.onConnectionStateChange?.(false);
    callbacks.current.onHistoryReplayStateChange?.(false);

    const connect = async () => {
      let id = ptyId;
      if (!id) {
        const cwd = consumePendingCwd(tabId) || tabCwdRef.current || undefined;
        id = await invoke<string>("spawn_pty", {
          cols: Math.max(term.cols, 80), rows: Math.max(term.rows, 24),
          predictedId: uid(), cwd, tabId,
        });
        if (disposed) return;
        callbacks.current.onPtySpawned?.(tabId, id);
        callbacks.current.onShellReady();
      }
      if (disposed) return;
      connectedPtyRef.current = id;
      const sink: TabOutputSink = {
        onReady() { if (!disposed) callbacks.current.onConnectionStateChange?.(true); },
        onReplayComplete() {
          if (disposed) return;
          callbacks.current.onConnectionStateChange?.(true);
          return writeTerminalOutputAndWait(term, "", () => {
            if (termRef.current === term) callbacks.current.onFrameReady?.(id);
          });
        },
        onOutput: (text) => {
          if (disposed) return;
          const written = writeTerminalOutputAndWait(term, text, () => {
            if (termRef.current === term) callbacks.current.onFrameReady?.(id);
          });
          callbacks.current.onOutputChunk(text);
          return written;
        },
        onBytes: (bytes) => {
          if (disposed) return;
          const written = writeTerminalOutputAndWait(term, bytes, () => {
            if (termRef.current === term) callbacks.current.onFrameReady?.(id);
          });
          callbacks.current.onOutputChunk(new TextDecoder().decode(bytes.slice(-200)));
          return written;
        },
        onTruncated: (from) => {
          if (!disposed) term.writeln(`\r\n\x1b[90m[output truncated — resuming from byte ${from}]\x1b[0m`);
        },
        onEnded: () => {
          if (disposed) return;
          connectedPtyRef.current = null;
          callbacks.current.onConnectionStateChange?.(false);
        },
        onStreamError: () => {
          if (!disposed) callbacks.current.onConnectionStateChange?.(false);
        },
      };
      unsubscribe = subscribeTabOutput(tabId, sink, term, id);
    };
    void connect().catch((error: unknown) => {
      if (disposed) return;
      if (isPlanLockedRefusal(error)) {
        window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
        callbacks.current.onConnectionStateChange?.(false);
        return;
      }
      console.warn("Failed to setup PTY:", error);
      callbacks.current.onConnectionStateChange?.(false);
    });
    return () => {
      disposed = true;
      unsubscribe?.();
      connectedPtyRef.current = null;
      callbacks.current.onConnectionStateChange?.(false);
    };
  }, [ptyId, tabId, termReady, ioEnabled, renderEnabled, connectedPtyRef, termRef]);

  return { writePty };
}
