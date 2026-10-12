import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "../../lib/bridge";
import { subscribeTabOutput, type TabOutputSink } from "../../lib/outputStream";
import { useMuxSelector } from "../../store/multiplexer";
import { consumePendingCwd } from "../../lib/pendingCommands";
import { uid } from "../../lib/uid";
import { writeTerminalOutput } from "./terminalOutputWrite";
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
  onConnectionStateChange,
}: UsePtyConnectionOptions): UsePtyConnectionReturn {
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
    if (!term || !termReady || !ioEnabled) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    onConnectionStateChange?.(false);
    onHistoryReplayStateChange?.(false);

    const connect = async () => {
      let id = ptyId;
      if (!id) {
        const cwd = consumePendingCwd(tabId) || tabCwdRef.current || undefined;
        id = await invoke<string>("spawn_pty", {
          cols: Math.max(term.cols, 80), rows: Math.max(term.rows, 24),
          predictedId: uid(), cwd, tabId,
        });
        if (disposed) return;
        onPtySpawned?.(tabId, id);
        onShellReady();
      }
      if (disposed) return;
      connectedPtyRef.current = id;
      const sink: TabOutputSink = {
        onReady() { if (!disposed) onConnectionStateChange?.(true); },
        onOutput: (text) => {
          if (disposed) return;
          writeTerminalOutput(term, text);
          onOutputChunk(text);
        },
        onBytes: (bytes) => {
          if (disposed) return;
          writeTerminalOutput(term, bytes);
          onOutputChunk(new TextDecoder().decode(bytes.slice(-200)));
        },
        onTruncated: (from) => {
          if (!disposed) term.writeln(`\r\n\x1b[90m[output truncated — resuming from byte ${from}]\x1b[0m`);
        },
        onEnded: () => {
          if (disposed) return;
          connectedPtyRef.current = null;
          onConnectionStateChange?.(false);
        },
        onStreamError: () => {
          if (!disposed) onConnectionStateChange?.(false);
        },
      };
      unsubscribe = subscribeTabOutput(tabId, sink, term, id);
    };
    void connect().catch((error: unknown) => {
      if (disposed) return;
      console.warn("Failed to setup PTY:", error);
      onConnectionStateChange?.(false);
    });
    return () => {
      disposed = true;
      unsubscribe?.();
      connectedPtyRef.current = null;
      onConnectionStateChange?.(false);
    };
  }, [ptyId, tabId, termReady, ioEnabled, onPtySpawned, onConnectionStateChange, onHistoryReplayStateChange, onShellReady, onOutputChunk, connectedPtyRef, termRef]);

  return { writePty };
}
