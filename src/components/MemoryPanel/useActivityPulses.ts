import { useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { baseUrl, authHeaders, isTauriEnv } from "./memoryApi";
import { hashString } from "./seedLayout";

export type MemOp =
  | "read"
  | "search"
  | "write"
  | "update"
  | "delete"
  | "recall"
  | "dream";

export interface MemActivitySource {
  kind: "cli" | "rest" | "dream" | "worker";
  pane_id: string | null;
  worker_id: string | null;
  agent_provider: string | null;
}

export interface MemActivityEvent {
  op: MemOp;
  mem_ids: string[];
  source: MemActivitySource;
  ts_ms: number;
}

export interface Pulse {
  color: string;
  op: MemOp;
  startTime: number;
  duration: number;
}

const PULSE_DURATION_MS = 3000;
const MAX_RECENT_EVENTS = 20;
const POLL_INTERVAL_MS = 1000;

const PALETTE_HUES = [0, 36, 72, 108, 144, 180, 216, 252, 288, 324];

export function colorForSource(source: MemActivitySource): string {
  const key = source.worker_id || source.pane_id || source.kind || "unknown";
  const hue = PALETTE_HUES[hashString(key) % PALETTE_HUES.length];
  return `hsl(${hue}, 70%, 55%)`;
}

export interface UseActivityPulsesResult {
  pulses: Map<string, Pulse>;
  recentEvents: MemActivityEvent[];
}

export function useActivityPulses(api: ApiConfig): UseActivityPulsesResult {
  const pulsesRef = useRef<Map<string, Pulse>>(new Map());
  const [, setTick] = useState(0);
  const [recentEvents, setRecentEvents] = useState<MemActivityEvent[]>([]);

  const forceRender = () => setTick((t) => (t + 1) & 0x7fffffff);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    let pollTimer: ReturnType<typeof setInterval> | undefined;
    let lastTs = Date.now();
    let rafHandle: number | null = null;

    const handleEvent = (ev: MemActivityEvent) => {
      if (!active) return;
      const color = colorForSource(ev.source);
      const now = performance.now();
      for (const id of ev.mem_ids) {
        pulsesRef.current.set(id, {
          color,
          op: ev.op,
          startTime: now,
          duration: PULSE_DURATION_MS,
        });
      }
      if (ev.mem_ids.length > 0) {
        setRecentEvents((prev) => {
          const next = [ev, ...prev];
          if (next.length > MAX_RECENT_EVENTS) next.length = MAX_RECENT_EVENTS;
          return next;
        });
      }
      if (ev.ts_ms > lastTs) lastTs = ev.ts_ms;
      scheduleLoop();
      forceRender();
    };

    const loop = () => {
      rafHandle = null;
      if (!active) return;
      const map = pulsesRef.current;
      if (map.size === 0) return;
      const now = performance.now();
      let changed = false;
      for (const [id, p] of map) {
        if (now - p.startTime > p.duration) {
          map.delete(id);
          changed = true;
        }
      }
      if (changed) forceRender();
      if (map.size > 0) {
        rafHandle = requestAnimationFrame(loop);
      }
    };

    const scheduleLoop = () => {
      if (rafHandle == null && pulsesRef.current.size > 0) {
        rafHandle = requestAnimationFrame(loop);
      }
    };

    const setup = async () => {
      if (isTauriEnv()) {
        try {
          const fn = await listen<MemActivityEvent>("mem-activity", (ev) => {
            if (!active) return;
            handleEvent(ev.payload);
          });
          if (!active) {
            fn();
            return;
          }
          unlisten = fn;
        } catch {
          startPolling();
        }
      } else {
        startPolling();
      }
    };
    setup();

    function startPolling() {
      if (pollTimer != null) return;
      pollTimer = setInterval(async () => {
        if (!active) return;
        try {
          const res = await fetch(
            `${baseUrl(api)}/api/mem/activity/recent?since_ms=${lastTs}`,
            { headers: authHeaders(api) },
          );
          if (!res.ok) return;
          const data = (await res.json()) as { events?: MemActivityEvent[] };
          const events = data.events ?? [];
          for (const ev of events) handleEvent(ev);
        } catch {
          // ignore, keep polling
        }
      }, POLL_INTERVAL_MS);
    }

    return () => {
      active = false;
      if (unlisten) unlisten();
      if (pollTimer != null) clearInterval(pollTimer);
      if (rafHandle != null) cancelAnimationFrame(rafHandle);
    };
  }, [api]);

  return { pulses: pulsesRef.current, recentEvents };
}
