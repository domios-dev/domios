import { useEffect, useMemo, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import type { ApiConfig } from "../../lib/types";
import { localFetch } from "../../lib/webApi";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { subscribeAgentRunOutput } from "./agentRunOutput";
import {
  ClaudeStreamParser,
  mergeAssistantMessages,
  reconcileToolInvocations,
} from "./streamParser";
import type {
  ClaudeStreamEvent,
  RawLine,
  ToolInvocationState,
} from "./streamParser";

interface UseAgentRunStreamOpts {
  api: ApiConfig;
  tabId: string | null | undefined;
  enabled?: boolean;
}

export interface UseAgentRunStreamResult {
  events: ClaudeStreamEvent[];
  invocations: ToolInvocationState[];
  rawLines: RawLine[];
  connected: boolean;
  error: string | null;
  reload: () => void;
}

export function useAgentRunStream({
  api,
  tabId,
  enabled = true,
}: UseAgentRunStreamOpts): UseAgentRunStreamResult {
  const [events, setEvents] = useState<ClaudeStreamEvent[]>([]);
  const [rawLines, setRawLines] = useState<RawLine[]>([]);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  const parserRef = useRef<ClaudeStreamParser | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (!enabled || !tabId) return;

    const parser = new ClaudeStreamParser();
    parserRef.current = parser;
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setEvents([]);
    setRawLines([]);
    setError(null);
    setConnected(false);

    let cancelled = false;

    // Throttle updates: partial-message mode can emit hundreds of deltas/sec.
    // Batch for ~120ms to avoid WebView2 IPC overload and UI freezes.
    let pendingEvents: ClaudeStreamEvent[] = [];
    let pendingRawLines: RawLine[] = [];
    let flushTimer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      flushTimer = null;
      if (cancelled) return;
      if (pendingEvents.length > 0) {
        const batch = pendingEvents;
        pendingEvents = [];
        setEvents((prev) => mergeAssistantMessages([...prev, ...batch]));
      }
      if (pendingRawLines.length > 0) {
        const batch = pendingRawLines;
        pendingRawLines = [];
        setRawLines((prev) => [...prev, ...batch]);
      }
    };
    const scheduleFlush = () => {
      if (flushTimer !== null || cancelled) return;
      flushTimer = setTimeout(flush, 120);
    };
    const pushParseResult = (events: ClaudeStreamEvent[], rawLines: RawLine[]) => {
      if (events.length === 0 && rawLines.length === 0) return;
      pendingEvents.push(...events);
      pendingRawLines.push(...rawLines);
      scheduleFlush();
    };

    const fallbackToHistorical = async () => {
    // If daemon restarts, live streaming stops but on-disk logs remain.
    // Fetch once and parse historical log content.
      try {
        const r = await localFetch(`/api/agent-runs/${encodeURIComponent(tabId)}/log`, { signal: ctrl.signal });
        if (cancelled) return;
        if (!r.ok) {
          setError("(no historical log available)");
          return;
        }
        const text = await r.text();
        if (cancelled) return;
        const result = parser.feed(text);
        pushParseResult(result.events, result.rawLines);
        setError(null);
      } catch (error) {
        if (cancelled || ctrl.signal.aborted) return;
        if (DEV_BUILD) console.debug("Background agent run history refresh failed", error);
        setError("(failed to load historical log)");
      }
    };

    const stop = subscribeAgentRunOutput(baseDaemonTransport(), tabId, ctrl.signal, {
      output(text) {
        const result = parser.feed(text);
        pushParseResult(result.events, result.rawLines);
      },
      connected: setConnected,
      ended(end, offset) {
        if (end.reason === "http" && end.status === 404 && offset === 0) {
          void fallbackToHistorical();
          return;
        }
        setError(end.error instanceof Error ? end.error.message : `stream ended: ${end.reason}`);
      },
    });

    return () => {
      cancelled = true;
      stop();
      if (flushTimer !== null) {
        clearTimeout(flushTimer);
        flushTimer = null;
      }
      ctrl.abort();
      abortRef.current = null;
      parserRef.current = null;
    };
  }, [api.host, api.port, api.token, tabId, enabled, reloadKey]);

  const invocations = useMemo(
    () => reconcileToolInvocations(events),
    [events],
  );

  return {
    events,
    invocations,
    rawLines,
    connected,
    error,
    reload: () => setReloadKey((k) => k + 1),
  };
}
