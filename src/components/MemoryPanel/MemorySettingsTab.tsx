import { useCallback, useEffect, useMemo, useState } from "react";
import { Cog, Moon, RefreshCw } from "lucide-react";
import type { AppConfig, CronJob } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import { Label, SettingsCard, SettingsPage, Toggle } from "../Settings/shared";
import { DecisionRerankCard } from "./DecisionRerankCard";
import { resolveWorkerSelection, type WorkerSelection } from "./workerSelection";

type ScheduleFreq = "daily" | "every-2" | "every-3" | "weekly" | "off";

function pad2(n: number): string {
  return n.toString().padStart(2, "0");
}

function scheduleToCron(freq: ScheduleFreq, time: string): string | null {
  if (freq === "off") return null;
  const [hh, mm] = time.split(":").map((n) => parseInt(n, 10));
  if (!Number.isFinite(hh) || !Number.isFinite(mm)) return null;
  const m = mm;
  const h = hh;
  switch (freq) {
    case "daily": return `${m} ${h} * * *`;
    case "every-2": return `${m} ${h} */2 * *`;
    case "every-3": return `${m} ${h} */3 * *`;
    case "weekly": return `${m} ${h} * * 0`;
  }
}

function cronToSchedule(schedule: string): { freq: ScheduleFreq; time: string; custom: boolean } {
  const s = schedule.trim();
  let m: RegExpMatchArray | null;
  m = s.match(/^(\d+) (\d+) \* \* \*$/);
  if (m) return { freq: "daily", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \*\/2 \* \*$/);
  if (m) return { freq: "every-2", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \*\/3 \* \*$/);
  if (m) return { freq: "every-3", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \* \* 0$/);
  if (m) return { freq: "weekly", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  return { freq: "daily", time: "06:00", custom: true };
}

function freqLabel(freq: ScheduleFreq): string {
  switch (freq) {
    case "daily": return "Daily";
    case "every-2": return "Every 2 days";
    case "every-3": return "Every 3 days";
    case "weekly": return "Weekly (Sunday)";
    case "off": return "Off";
  }
}

function relativeFromNow(target: number): string {
  const diff = target - Date.now();
  if (diff <= 0) return "soon";
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `in about ${mins} minute${mins === 1 ? "" : "s"}`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `about ${hrs} hour${hrs === 1 ? "" : "s"} from now`;
  const days = Math.floor(hrs / 24);
  return `about ${days} day${days === 1 ? "" : "s"} from now`;
}

interface MemorySettingsTabProps {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenAgentsSettings: () => void;
}

export function MemorySettingsTab({ config, onUpdate, onOpenAgentsSettings }: MemorySettingsTabProps) {
  const { providers, loading: modelCatalogLoading } = useAgentProviders();
  const inputClass = "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  const defaultProvider = config.general_agent?.agent_provider || providers[0]?.id || "";
  const defaultMem = {
    extraction_agent: "",
    extraction_model: "",
    auto_extract: true,
    dreaming_agent: "",
    dreaming_model: "",
    worker_model: "",
    worker_timeout_seconds: 1800,
    file_sync_enabled: true,
    file_sync_interval_minutes: 60,
    dedup_threshold: 0.15,
  };
  const mem = { ...defaultMem, ...(config.memory ?? {}) };

  const extractionSelection = resolveWorkerSelection(
    providers, mem.extraction_agent, defaultProvider, mem.extraction_model,
  );
  const dreamingSelection = resolveWorkerSelection(
    providers, mem.dreaming_agent, defaultProvider, mem.dreaming_model,
  );

  const [syncStatus, setSyncStatus] = useState<{ tracked_files: number; stale_files: number } | null>(null);
  const [syncing, setSyncing] = useState(false);

  const api = config.api;
  // daemonHttpBase safely defaults host/port, so never gate fetches on a
  // not-yet-populated config.api — that left the schedule dropdown stuck Off.
  const apiBase = daemonHttpBase(api);
  const authToken = daemonAuthToken(api);
  const apiHeaders: HeadersInit = authToken ? { Authorization: `Bearer ${authToken}` } : {};

  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [scheduleSaving, setScheduleSaving] = useState(false);

  useEffect(() => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/mem/sync/status`, { headers: apiHeaders })
      .then(r => r.json())
      .then(d => setSyncStatus(d))
      .catch(() => {});
  }, [apiBase]);

  useEffect(() => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/crons`, { headers: apiHeaders })
      .then(r => r.ok ? r.json() : [])
      .then((jobs: CronJob[]) => {
        const job = jobs.find(j => j.name === "mem-dreaming-auto");
        if (job) setDreamCron(job);
      })
      .catch(() => {});
  }, [apiBase]);

  const parsed = useMemo(
    () => dreamCron ? cronToSchedule(dreamCron.schedule) : null,
    [dreamCron],
  );
  const currentFreq: ScheduleFreq = !dreamCron
    ? "off"
    : !dreamCron.enabled
      ? "off"
      : parsed?.freq ?? "daily";
  const currentTime = parsed?.time ?? "06:00";

  const applySchedule = useCallback(async (freq: ScheduleFreq, time: string) => {
    if (!apiBase || !dreamCron) return;
    setScheduleSaving(true);
    try {
      if (freq === "off") {
        const r = await fetch(`${apiBase}/api/crons/${encodeURIComponent(dreamCron.id)}/disable`, {
          method: "POST",
          headers: apiHeaders,
        });
        if (r.ok) setDreamCron(await r.json() as CronJob);
        return;
      }
      const cronStr = scheduleToCron(freq, time);
      if (!cronStr) return;
      let updated: CronJob | null = null;
      if (cronStr !== dreamCron.schedule) {
        const r = await fetch(`${apiBase}/api/crons/${encodeURIComponent(dreamCron.id)}`, {
          method: "PUT",
          headers: { ...apiHeaders, "Content-Type": "application/json" },
          body: JSON.stringify({ schedule: cronStr }),
        });
        if (r.ok) updated = await r.json() as CronJob;
      } else {
        updated = dreamCron;
      }
      if (updated && !updated.enabled) {
        const r2 = await fetch(`${apiBase}/api/crons/${encodeURIComponent(dreamCron.id)}/enable`, {
          method: "POST",
          headers: apiHeaders,
        });
        if (r2.ok) updated = await r2.json() as CronJob;
      }
      if (updated) setDreamCron(updated);
    } catch { /* ignore */ }
    finally { setScheduleSaving(false); }
  }, [apiBase, dreamCron]);

  const triggerSync = useCallback(async () => {
    if (!apiBase) return;
    setSyncing(true);
    try {
      const r = await fetch(`${apiBase}/api/mem/sync`, { method: "POST", headers: { "Content-Type": "application/json", ...apiHeaders }, body: "{}" });
      const d = await r.json();
      setSyncStatus({ tracked_files: d.total_tracked ?? 0, stale_files: d.source_missing ?? 0 });
    } catch { /* ignore */ }
    setSyncing(false);
  }, [apiBase]);

  const triggerDream = useCallback(async () => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/mem/dream`, { method: "POST", headers: { "Content-Type": "application/json", ...apiHeaders }, body: "{}" }).catch(() => {});
  }, [apiBase]);

  const renderProviderSelect = (
    value: string,
    onChange: (v: string) => void,
    selection: WorkerSelection,
    testId: string,
  ) => (
    <Label text="Provider">
      <select value={value || ""} onChange={(e) => onChange(e.target.value)} className={inputClass} data-testid={testId}>
        <option value="">Default: {selection.providerLabel} / {selection.defaultModelLabel}</option>
        {providers.map(p => (
          <option key={p.id} value={p.id}>{p.display_name}</option>
        ))}
      </select>
    </Label>
  );

  const renderModelSelect = (
    value: string,
    onChange: (v: string) => void,
    selection: WorkerSelection,
    testId: string,
  ) => {
    const unavailable = Boolean(value) && !selection.modelCompatible;
    const modelOptions = modelCatalogLoading ? [] : selection.modelOptions;
    const disabled = modelOptions.length === 0;
    const emptyLabel = modelCatalogLoading
      ? "Loading model catalog…"
      : modelOptions.length === 0
        ? "No models published"
        : `Provider default: ${selection.defaultModelLabel}`;
    return (
      <Label text="Model">
        <select
          value={unavailable ? "" : value}
          onChange={(e) => onChange(e.target.value)}
          className={inputClass}
          data-testid={testId}
          disabled={disabled}
          aria-busy={modelCatalogLoading}
        >
          <option value="">{emptyLabel}</option>
          {modelOptions.map((model) => (
            <option key={model.id} value={model.id}>{model.display_name}</option>
          ))}
        </select>
        {unavailable && !modelCatalogLoading && (
          <p className="text-[10px] text-amber-500" role="alert">
            The configured model is unavailable for {selection.providerLabel}; choose a catalog model or use {selection.defaultModelLabel}.
          </p>
        )}
      </Label>
    );
  };

  const dreamRunButton = (
    <button
      onClick={triggerDream}
      data-testid="memory-settings-run-dream"
      className="text-[10px] uppercase tracking-wider px-2.5 py-1 rounded bg-[var(--ct-accent)] text-white hover:opacity-80"
    >
      Run Now
    </button>
  );

  const syncRunButton = (
    <button
      onClick={triggerSync}
      disabled={syncing}
      data-testid="memory-settings-sync-now"
      className="text-[10px] uppercase tracking-wider px-2.5 py-1 rounded bg-[var(--ct-accent)] text-white hover:opacity-80 disabled:opacity-50"
    >
      {syncing ? "Syncing..." : "Sync Now"}
    </button>
  );

  return (
    <SettingsPage title="Memory" subtitle="Extraction, dreaming, and file sync.">
      <SettingsCard
        icon={<Cog size={16} />}
        title="Extraction"
        description="On save, headless workers extract entities, relations, and decisions into the knowledge graph."
      >
        <Toggle
          label="Auto-extract on save"
          checked={mem.auto_extract}
          onChange={(v) => onUpdate("memory", { ...mem, auto_extract: v })}
        />
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {renderProviderSelect(
            mem.extraction_agent,
            (v) => onUpdate("memory", { ...mem, extraction_agent: v }),
            extractionSelection,
            "memory-settings-extraction-provider",
          )}
          {renderModelSelect(
            mem.extraction_model,
            (v) => onUpdate("memory", { ...mem, extraction_model: v }),
            extractionSelection,
            "memory-settings-extraction-model",
          )}
          <Label text="Dedup threshold">
            <input
              type="number"
              min={0.05}
              max={0.5}
              step={0.01}
              value={mem.dedup_threshold}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n > 0) {
                  onUpdate("memory", { ...mem, dedup_threshold: n });
                }
              }}
              className={inputClass}
              data-testid="memory-settings-dedup-threshold"
            />
          </Label>
          <Label text="Worker timeout (s)">
            <input
              type="number"
              min={10}
              max={3600}
              value={mem.worker_timeout_seconds}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n > 0) {
                  onUpdate("memory", { ...mem, worker_timeout_seconds: n });
                }
              }}
              className={inputClass}
              data-testid="memory-settings-worker-timeout"
            />
          </Label>
        </div>
        <p className="text-[10px] text-[var(--ct-muted)]">
          Dedup: cosine distance below this triggers dedup. Lower = stricter (default 0.15).
        </p>
      </SettingsCard>

      <SettingsCard
        icon={<Moon size={16} />}
        title="Dreaming (Nightly Optimizer)"
        headerRight={dreamRunButton}
        description="LLM consolidates, dedups, promotes learnings, prunes stale relations. Runs nightly by default."
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 items-end">
          <Label text="Schedule">
            <select
              data-testid="memory-settings-dream-schedule"
              value={currentFreq}
              disabled={!dreamCron || scheduleSaving}
              onChange={(e) => applySchedule(e.target.value as ScheduleFreq, currentTime)}
              className={inputClass}
            >
              {(["daily", "every-2", "every-3", "weekly", "off"] as ScheduleFreq[]).map(f => (
                <option key={f} value={f}>{freqLabel(f)}</option>
              ))}
            </select>
          </Label>
          <Label text="At">
            <input
              type="time"
              value={currentTime}
              disabled={!dreamCron || currentFreq === "off" || scheduleSaving}
              onChange={(e) => applySchedule(currentFreq === "off" ? "daily" : currentFreq, e.target.value)}
              className={inputClass}
              data-testid="memory-settings-dream-time"
            />
          </Label>
        </div>
        {dreamCron && (
          <p className="text-[10px] text-[var(--ct-muted)]">
            {currentFreq === "off"
              ? "Disabled — no automatic runs."
              : dreamCron.enabled && dreamCron.next_run
                ? `Next run ${relativeFromNow(dreamCron.next_run)} · Runs ${freqLabel(currentFreq)} at ${currentTime}`
                : `Runs ${freqLabel(currentFreq)} at ${currentTime}`}
            {parsed?.custom && (
              <span className="block opacity-70 mt-0.5">custom: {dreamCron.schedule}</span>
            )}
          </p>
        )}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {renderProviderSelect(
            mem.dreaming_agent,
            (v) => onUpdate("memory", { ...mem, dreaming_agent: v }),
            dreamingSelection,
            "memory-settings-dreaming-provider",
          )}
          {renderModelSelect(
            mem.dreaming_model,
            (v) => onUpdate("memory", { ...mem, dreaming_model: v }),
            dreamingSelection,
            "memory-settings-dreaming-model",
          )}
        </div>
      </SettingsCard>

      <SettingsCard
        icon={<RefreshCw size={16} />}
        title="File Sync (Librarian)"
        headerRight={syncRunButton}
        description={
          <>
            Re-vectorizes source-tracked files when they change on disk. No LLM.
            {syncStatus && (
              <> · Tracked: {syncStatus.tracked_files}{syncStatus.stale_files > 0 ? ` (${syncStatus.stale_files} stale)` : ""}</>
            )}
          </>
        }
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 items-end">
          <Toggle
            label="Periodic sync"
            checked={mem.file_sync_enabled}
            onChange={(v) => onUpdate("memory", { ...mem, file_sync_enabled: v })}
          />
          <Label text="Interval (min)">
            <input
              type="number"
              min={5}
              max={1440}
              value={mem.file_sync_interval_minutes}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n >= 5) {
                  onUpdate("memory", { ...mem, file_sync_interval_minutes: n });
                }
              }}
              className={inputClass}
              data-testid="memory-settings-file-sync-interval"
            />
          </Label>
        </div>
      </SettingsCard>

      <DecisionRerankCard supporter={config.supporter} onUpdate={onUpdate} onOpenAgentsSettings={onOpenAgentsSettings} />
    </SettingsPage>
  );
}
