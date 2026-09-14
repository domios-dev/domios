import { useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
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

// Use fetch+ReadableStream so bearer token headers are sent.
// `from=0` replays existing PTY history, then switches to live stream.
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

    const url = `${daemonHttpBase(api)}/api/tabs/${encodeURIComponent(tabId)}/output/stream?from=0`;
    const token = daemonAuthToken(api);
    const headers: HeadersInit = token
      ? { Authorization: `Bearer ${token}` }
      : {};

    const fallbackToHistorical = async () => {
    // If daemon restarts, live streaming stops but on-disk logs remain.
    // Fetch once and parse historical log content.
      try {
        const histUrl = `${daemonHttpBase(api)}/api/agent-runs/${encodeURIComponent(tabId)}/log`;
        const r = await fetch(histUrl, { headers, signal: ctrl.signal });
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
      } catch {
        setError("(failed to load historical log)");
      }
    };

    (async () => {
      try {
        const res = await fetch(url, { headers, signal: ctrl.signal });
        if (!res.ok || !res.body) {
          if (res.status === 404) {
            await fallbackToHistorical();
            return;
          }
          throw new Error(`stream open failed: ${res.status}`);
        }
        setConnected(true);
        const reader = res.body.getReader();
        const decoder = new TextDecoder("utf-8", { fatal: false });
        let sseTail = "";

        while (!cancelled) {
          const { value, done } = await reader.read();
          if (done) break;
          if (!value) continue;

          sseTail += decoder.decode(value, { stream: true });

          const frames = sseTail.split("\n\n");
          sseTail = frames.pop() ?? "";

          let chunkText = "";
          for (const frame of frames) {
            for (const line of frame.split("\n")) {
              if (line.startsWith("data:")) {
                const data = line.slice(5).trim();
                if (!data) continue;
                try {
                  const parsed = JSON.parse(data) as { output?: string };
                  if (typeof parsed.output === "string") {
                    chunkText += parsed.output;
                  }
                } catch {
                  chunkText += data;
                }
              }
            }
          }

          if (chunkText) {
            const result = parser.feed(chunkText);
            pushParseResult(result.events, result.rawLines);
          }
        }
      } catch (e) {
        if (cancelled || (e as Error).name === "AbortError") return;
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setConnected(false);
      }
    })();

    return () => {
      cancelled = true;
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
