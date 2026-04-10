import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig, CronJob, CronRunInfo, CronRunStatus } from "../../lib/types";
import { baseUrl, authHeaders } from "./memoryApi";

interface Props {
  api: ApiConfig;
}

interface DreamDiaryEntry {
  id: string;
  title: string;
  body: string;
  created_at: string;
  area: string | null;
  tags?: string[];
}

interface SearchHit {
  item: DreamDiaryEntry;
  score: number;
}

function relativeTime(target: number): string {
  const diff = target - Date.now();
  const abs = Math.abs(diff);
  const past = diff < 0;
  const mins = Math.floor(abs / 60000);
  if (mins < 1) return past ? "just now" : "in <1m";
  if (mins < 60) return past ? `${mins}m ago` : `in ${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return past ? `${hrs}h ago` : `in ${hrs}h`;
  const days = Math.floor(hrs / 24);
  return past ? `${days}d ago` : `in ${days}d`;
}

function formatDuration(startMs: number, endMs: number): string {
  const diff = endMs - startMs;
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return `${mins}m ${rem}s`;
}

function formatDateTime(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function statusColor(status: CronRunStatus): string {
  switch (status) {
    case "Success": return "#10b981";
    case "Failed": return "#ef4444";
    case "Running": return "#eab308";
  }
}

function parseDreamStats(body: string): { promoted: number; expired: number } {
  let promoted = 0;
  let expired = 0;
  const promMatch = body.match(/promot\w*[:\s]+(\d+)/i);
  if (promMatch) promoted = parseInt(promMatch[1], 10);
  const expMatch = body.match(/expir\w*[:\s]+(\d+)/i);
  if (expMatch) expired = parseInt(expMatch[1], 10);
  return { promoted, expired };
}

function MemoryDreamsTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [cronLoading, setCronLoading] = useState(true);
  const [history, setHistory] = useState<CronRunInfo[]>([]);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const mountedRef = useRef(true);

  useEffect(() => {
    return () => { mountedRef.current = false; };
  }, []);

  const fetchCronData = useCallback(async () => {
    setCronLoading(true);
    try {
      const res = await fetch(`${baseUrl(api)}/api/crons`, { headers: authHeaders(api) });
      if (!res.ok) return;
      const jobs = (await res.json()) as CronJob[];
      const dream = jobs.find(j => j.name === "mem-dreaming-auto") ?? null;
      if (!mountedRef.current) return;
      setDreamCron(dream);

      if (dream) {
        const histRes = await fetch(`${baseUrl(api)}/api/crons/${encodeURIComponent(dream.id)}/history`, {
          headers: authHeaders(api),
        });
        if (histRes.ok && mountedRef.current) {
          const runs = (await histRes.json()) as CronRunInfo[];
          runs.sort((a, b) => b.started_at - a.started_at);
          setHistory(runs);
        }
      }
    } catch {
      // network error, leave defaults
    } finally {
      if (mountedRef.current) setCronLoading(false);
    }
  }, [api]);

  const fetchDiary = useCallback(async () => {
    setDiaryLoading(true);
    try {
      const params = new URLSearchParams({
        q: "dream-diary",
        limit: "20",
      });
      const res = await fetch(`${baseUrl(api)}/api/mem/search?${params}`, {
        headers: authHeaders(api),
      });
      if (!res.ok || !mountedRef.current) return;
      const data = (await res.json()) as { hits: SearchHit[] };
      const entries = (data.hits || [])
        .map(h => h.item)
        .filter(i => i.tags?.includes("dream-diary"));
      entries.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
      setDiaryEntries(entries);
    } catch {
      // network error
    } finally {
      if (mountedRef.current) setDiaryLoading(false);
    }
  }, [api]);

  useEffect(() => { fetchCronData(); }, [fetchCronData]);
  useEffect(() => { fetchDiary(); }, [fetchDiary]);

  const aggregateStats = diaryEntries.reduce(
    (acc, e) => {
      const s = parseDreamStats(e.body);
      return { promoted: acc.promoted + s.promoted, expired: acc.expired + s.expired };
    },
    { promoted: 0, expired: 0 },
  );

  return (
    <div className="absolute inset-0 flex flex-col overflow-y-auto">
      <div className="p-3 space-y-4">
        {/* Next dream countdown */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Next Dream
          </h3>
          {cronLoading ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">Loading...</div>
          ) : !dreamCron ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">
              No dreaming cron found. Memory dreaming is not configured.
            </div>
          ) : (
            <div className="flex items-center gap-3 px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]">
              <div className="flex-1">
                <div className="text-sm text-[var(--ct-fg)] font-mono">
                  {dreamCron.next_run
                    ? relativeTime(dreamCron.next_run)
                    : "Not scheduled"}
                </div>
                {dreamCron.next_run && (
                  <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5">
                    {new Date(dreamCron.next_run).toLocaleString("en-US", {
                      weekday: "short",
                      month: "short",
                      day: "numeric",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </div>
                )}
              </div>
              <span
                className="px-2 py-0.5 rounded text-[10px] font-semibold uppercase"
                style={{
                  background: dreamCron.enabled ? "#10b98122" : "#ef444422",
                  color: dreamCron.enabled ? "#10b981" : "#ef4444",
                }}
              >
                {dreamCron.enabled ? "enabled" : "disabled"}
              </span>
            </div>
          )}
        </section>

        {/* Stats summary */}
        {diaryEntries.length > 0 && (
          <section>
            <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
              Lifetime Stats
            </h3>
            <div className="grid grid-cols-3 gap-2">
              <div className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] text-center">
                <div className="text-lg font-mono text-[var(--ct-fg)]">{diaryEntries.length}</div>
                <div className="text-[10px] text-[var(--ct-muted-fg)]">dream runs</div>
              </div>
              <div className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] text-center">
                <div className="text-lg font-mono text-[#10b981]">{aggregateStats.promoted}</div>
                <div className="text-[10px] text-[var(--ct-muted-fg)]">promoted</div>
              </div>
              <div className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] text-center">
                <div className="text-lg font-mono text-[#ef4444]">{aggregateStats.expired}</div>
                <div className="text-[10px] text-[var(--ct-muted-fg)]">expired</div>
              </div>
            </div>
          </section>
        )}

        {/* Run history */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Run History
          </h3>
          {history.length === 0 && !cronLoading && (
            <div className="text-xs text-[var(--ct-muted-fg)]">No dream runs yet</div>
          )}
          <div className="space-y-1">
            {history.map(run => (
              <div
                key={run.run_id}
                className="flex items-center gap-2.5 px-3 py-1.5 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]"
              >
                <span
                  className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase tracking-[0.3px] w-[58px] text-center shrink-0"
                  style={{
                    background: `${statusColor(run.status)}22`,
                    color: statusColor(run.status),
                  }}
                >
                  {run.status}
                </span>
                <span className="text-[11px] text-[var(--ct-fg)] font-mono tabular-nums">
                  {formatDateTime(run.started_at)}
                </span>
                {run.finished_at && (
                  <span className="text-[10px] text-[var(--ct-muted-fg)] ml-auto tabular-nums">
                    {formatDuration(run.started_at, run.finished_at)}
                  </span>
                )}
                {!run.finished_at && run.status === "Running" && (
                  <span className="text-[10px] text-[#eab308] ml-auto flex items-center gap-1">
                    <span className="inline-block w-1.5 h-1.5 rounded-full bg-[#eab308] animate-pulse" />
                    running
                  </span>
                )}
              </div>
            ))}
          </div>
        </section>

        {/* Dream diary */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Dream Diary
          </h3>
          {diaryLoading ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">Loading diary...</div>
          ) : diaryEntries.length === 0 ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">
              No dream diary entries. Dreams will appear here after the first dreaming run.
            </div>
          ) : (
            <div className="space-y-2">
              {diaryEntries.map(entry => (
                <div
                  key={entry.id}
                  className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]"
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[11px] font-semibold text-[var(--ct-fg)]">
                      {entry.title}
                    </span>
                    <span className="text-[10px] text-[var(--ct-muted-fg)] ml-auto tabular-nums">
                      {new Date(entry.created_at).toLocaleDateString("en-US", {
                        month: "short",
                        day: "numeric",
                        year: "numeric",
                      })}
                    </span>
                  </div>
                  <pre className="text-[11px] text-[var(--ct-muted-fg)] whitespace-pre-wrap font-sans leading-relaxed m-0">
                    {entry.body}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

export const MemoryDreamsTab = memo(MemoryDreamsTabImpl);
