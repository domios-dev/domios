import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { RefreshCw, Trash2, ScrollText } from "lucide-react";
import { BusySpinner } from "../shared/BusyState";
import type { AppConfig } from "../../lib/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import { SettingsCard } from "./shared";
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
export const LEVEL_TONE: Record<string, "green" | "amber" | "red" | "muted"> = {
  info: "green",
  warn: "amber",
  error: "red",
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
    <SettingsCard
      icon={<ScrollText size={16} />}
      title="App Logs"
      description="Diagnostics from the daemon, mesh, agents, and integrations. Filter by level, source, or time range. Updates live."
      headerRight={
        <button
          data-testid="settings-logs-refresh"
          onClick={refresh}
          disabled={loading}
          className="flex items-center gap-1 px-2 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors text-[11px] disabled:opacity-50"
          title="Refresh"
        >
          <BusySpinner icon={RefreshCw} size={12} spinning={loading} />
          <span>Refresh</span>
        </button>
      }
    >
      <LogsFilterBar
        levels={levels}
        onToggleLevel={toggleLevel}
        source={source}
        onSourceChange={setSource}
        sources={sourcesList}
        range={range}
        onRangeChange={setRange}
      />

      {error && (
        <div className="text-[10px] text-[var(--ct-red)]" data-testid="settings-logs-error" role="alert">Failed to load logs: {error}</div>
      )}

      <div className="max-h-[440px] overflow-y-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 divide-y divide-[var(--ct-border)]" data-testid="settings-logs-list">
        {visible.length === 0 ? (
          <div className="px-3 py-8 text-center text-[11px] text-[var(--ct-muted-fg)]" data-testid="settings-logs-empty">
            No logs match the current filters.
          </div>
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

      <div className="flex items-center justify-between gap-3 text-[10px] text-[var(--ct-muted-fg)]">
        <span>{`Showing ${visible.length} of ${logs.length} logs`}</span>
        <button
          data-testid="settings-logs-clear-old"
          onClick={clearOld}
          className="inline-flex items-center gap-1 text-[var(--ct-muted-fg)] hover:text-[var(--ct-red)] transition-colors"
          title="Delete logs older than 7 days"
        >
          <Trash2 size={11} />
          <span>Clear logs older than 7d</span>
        </button>
      </div>
    </SettingsCard>
  );
}
