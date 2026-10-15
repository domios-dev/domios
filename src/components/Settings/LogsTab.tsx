import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { RefreshCw, Trash2 } from "lucide-react";
import { BusySpinner } from "../shared/BusyState";
import type { AppConfig } from "../../lib/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import { LogsFilterBar } from "./LogsFilterBar";
import { LogsListRow } from "./LogsListRow";

export type LogLevel = "info" | "warn" | "error";
export type RangeKey = "1h" | "24h" | "7d" | "all";

export const ALL_LEVELS: LogLevel[] = ["info", "warn", "error"];
export const RANGE_OPTIONS: { key: RangeKey; label: string; ms: number | null }[] = [
  { key: "1h", label: "1h", ms: 3_600_000 },
  { key: "24h", label: "24h", ms: 86_400_000 },
  { key: "7d", label: "7d", ms: 7 * 86_400_000 },
  { key: "all", label: "All", ms: null },
];
export const LEVEL_COLOR: Record<LogLevel, string> = {
  info: "var(--ct-accent)",
  warn: "var(--ct-yellow)",
  error: "var(--ct-red)",
};

export interface AppLog {
  id: number;
  ts: number;
  level: string;
  source: string;
  message: string;
  context_json?: string | null;
}

interface LogEventPayload {
  ts: number;
  level: string;
  source: string;
  message: string;
  context?: unknown;
}

export function LogsTab({ config }: { config: AppConfig }) {
  const api = config.api;
  const apiBase = api ? daemonHttpBase(api) : "";
  const authHeaders: HeadersInit = useMemo(
    () => {
      const token = daemonAuthToken(api);
      return token ? { Authorization: `Bearer ${token}` } : ({} as Record<string, string>);
    },
    [api?.host, api?.port, api?.token],
  );

  const [logs, setLogs] = useState<AppLog[]>([]);
  const [levels, setLevels] = useState<Set<LogLevel>>(new Set(ALL_LEVELS));
  const [source, setSource] = useState<string>("");
  const [range, setRange] = useState<RangeKey>("24h");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const nextLocalId = useRef(-1);
  const knownSourcesRef = useRef<Set<string>>(new Set());
  const [sourcesList, setSourcesList] = useState<string[]>([]);

  const rangeMs = useMemo(() => RANGE_OPTIONS.find((r) => r.key === range)?.ms ?? null, [range]);

  const refresh = useCallback(async () => {
    if (!apiBase) return;
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams();
      params.set("limit", "500");
      if (rangeMs !== null) params.set("since", String(Date.now() - rangeMs));
      const r = await fetch(`${apiBase}/api/logs?${params.toString()}`, { headers: authHeaders });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const rows = (await r.json()) as AppLog[];
      setLogs(Array.isArray(rows) ? rows : []);
      const next = new Set(knownSourcesRef.current);
      for (const row of rows) next.add(row.source);
      knownSourcesRef.current = next;
      setSourcesList(Array.from(next).sort());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [apiBase, authHeaders, rangeMs]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<LogEventPayload>("logs-updated", (event) => {
      const p = event.payload;
      const ctx = p.context === undefined || p.context === null ? null : JSON.stringify(p.context);
      const incoming: AppLog = {
        id: nextLocalId.current--,
        ts: p.ts,
        level: p.level,
        source: p.source,
        message: p.message,
        context_json: ctx,
      };
      setLogs((prev) => [incoming, ...prev].slice(0, 1000));
      if (!knownSourcesRef.current.has(p.source)) {
        knownSourcesRef.current.add(p.source);
        setSourcesList(Array.from(knownSourcesRef.current).sort());
      }
    })
      .then((un) => {
        unlisten = un;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  const visible = useMemo(() => {
    const src = source.trim().toLowerCase();
    return logs.filter((r) => {
      const lvl = r.level.toLowerCase();
      if (lvl === "info" || lvl === "warn" || lvl === "error") {
        if (!levels.has(lvl as LogLevel)) return false;
      } else if (levels.size < ALL_LEVELS.length) {
        return false;
      }
      if (src && !r.source.toLowerCase().includes(src)) return false;
      return true;
    });
  }, [logs, levels, source]);

  const toggleLevel = useCallback((l: LogLevel) => {
    setLevels((prev) => {
      const next = new Set(prev);
      if (next.has(l)) {
        if (next.size === 1) return prev;
        next.delete(l);
      } else {
        next.add(l);
      }
      return next;
    });
  }, []);

  const toggleExpand = useCallback((id: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const clearOld = useCallback(async () => {
    if (!apiBase) return;
    const cutoff = Date.now() - 7 * 86_400_000;
    if (!window.confirm("Delete all logs older than 7 days?")) return;
    try {
      const r = await fetch(`${apiBase}/api/logs?before=${cutoff}`, {
        method: "DELETE",
        headers: authHeaders,
      });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [apiBase, authHeaders, refresh]);

  return (
    <section className="ct-logs" aria-label="App logs">
      <div className="ct-jobs-header ct-logs__toolbar">
        <LogsFilterBar
          levels={levels}
          onToggleLevel={toggleLevel}
          source={source}
          onSourceChange={setSource}
          sources={sourcesList}
          range={range}
          onRangeChange={setRange}
        />
        <button type="button" className="ct-bar-pill" data-testid="settings-logs-refresh" onClick={refresh} disabled={loading} aria-label="Refresh logs" title="Refresh">
          <BusySpinner icon={RefreshCw} size={13} spinning={loading} />
        </button>
      </div>

      {error && (
        <div data-testid="settings-logs-error"><InlineErrorNotice error={error} operation="loading logs" blocking /></div>
      )}

      <div className="ct-logs__list" data-testid="settings-logs-list">
        {visible.length === 0 ? (
          <p className="ct-logs__empty" data-testid="settings-logs-empty">No logs match the current filters.</p>
        ) : (
          visible.map((r) => (
            <LogsListRow
              key={r.id}
              row={r}
              expanded={expanded.has(r.id)}
              onToggle={() => toggleExpand(r.id)}
            />
          ))
        )}
      </div>

      <footer className="ct-logs__footer">
        <span>{`${visible.length} of ${logs.length} logs · updates live`}</span>
        <button type="button" className="ct-bar-pill" data-testid="settings-logs-clear-old" onClick={clearOld} title="Delete logs older than 7 days">
          <Trash2 size={12} aria-hidden="true" />
          Clear older than 7d
        </button>
      </footer>
    </section>
  );
}
