import { useCallback, useEffect, useState } from "react";
import type { AppConfig, CronJob } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { canonicalizeShort } from "../../lib/modelLabels";
import { Group, Label, Toggle } from "./shared";

export function MemoryTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const defaultMem = { extraction_agent: "", extraction_model: "haiku", auto_extract: true, dreaming_agent: "", dreaming_model: "haiku", worker_model: "haiku", worker_timeout_seconds: 1800, file_sync_enabled: true, file_sync_interval_minutes: 60, dedup_threshold: 0.15 };
  const rawMem = { ...defaultMem, ...(config.memory ?? {}) };
  const mem = {
    ...rawMem,
    extraction_model: canonicalizeShort(rawMem.extraction_model),
    dreaming_model: canonicalizeShort(rawMem.dreaming_model),
    worker_model: canonicalizeShort(rawMem.worker_model),
  };
  const { providers } = useAgentProviders();
  const inputClass = "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  const defaultProvider = config.general_agent?.agent_provider || "claude";

  const extractionProvider = mem.extraction_agent || defaultProvider;
  const extractionModels = providers.find(p => p.id === extractionProvider)?.available_models ?? [];

  const dreamingProvider = mem.dreaming_agent || defaultProvider;
  const dreamingModels = providers.find(p => p.id === dreamingProvider)?.available_models ?? [];

  const [syncStatus, setSyncStatus] = useState<{ tracked_files: number; stale_files: number } | null>(null);
  const [syncing, setSyncing] = useState(false);

  const api = config.api;
  const apiBase = api ? `http://${api.host === "0.0.0.0" ? "127.0.0.1" : api.host}:${api.port}` : "";
  const apiHeaders: HeadersInit = api?.token ? { Authorization: `Bearer ${api.token}` } : {};

  const [dreamCron, setDreamCron] = useState<{ id: string; enabled: boolean } | null>(null);

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
        if (job) setDreamCron({ id: job.id, enabled: job.enabled });
      })
      .catch(() => {});
  }, [apiBase]);

  const toggleDreamerCron = useCallback(async (enabled: boolean) => {
    if (!apiBase || !dreamCron) return;
    try {
      const r = await fetch(`${apiBase}/api/crons/${encodeURIComponent(dreamCron.id)}/${enabled ? "enable" : "disable"}`, {
        method: "POST",
        headers: apiHeaders,
      });
      if (r.ok) {
        const updated = (await r.json()) as CronJob;
        setDreamCron({ id: updated.id, enabled: updated.enabled });
      }
    } catch { /* ignore */ }
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
  ) => (
    <Label text="Provider">
      <select value={value || ""} onChange={(e) => onChange(e.target.value)} className={inputClass}>
        <option value="">Default ({defaultProvider})</option>
        {providers.map(p => (
          <option key={p.id} value={p.id}>{p.display_name}</option>
        ))}
      </select>
    </Label>
  );

  const renderModelSelect = (
    value: string,
    onChange: (v: string) => void,
    models: { id: string; display_name: string }[],
  ) => (
    <Label text="Model">
      {models.length > 0 ? (
        <select value={value} onChange={(e) => onChange(e.target.value)} className={inputClass}>
          <option value="">Provider default</option>
          {models.map(m => (
            <option key={m.id} value={m.id}>{m.display_name}</option>
          ))}
        </select>
      ) : (
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={inputClass}
          placeholder="haiku, sonnet, o3-mini..."
        />
      )}
    </Label>
  );

  return (
    <div className="space-y-5">
      <Group title="Extraction">
        <p className="text-[11px] text-[var(--ct-muted-fg)]">
          On save, headless workers extract entities, relations, and decisions into the knowledge graph.
        </p>
        <Toggle
          label="Auto-extract on save"
          checked={mem.auto_extract}
          onChange={(v) => onUpdate("memory", { ...mem, auto_extract: v })}
        />
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {renderProviderSelect(
            mem.extraction_agent,
            (v) => onUpdate("memory", { ...mem, extraction_agent: v }),
          )}
          {renderModelSelect(
            mem.extraction_model,
            (v) => onUpdate("memory", { ...mem, extraction_model: v }),
            extractionModels,
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
            />
          </Label>
        </div>
        <p className="text-[10px] text-[var(--ct-muted)]">
          Dedup: cosine distance below this triggers dedup. Lower = stricter (default 0.15).
        </p>
      </Group>

      <Group title="Periodic Tasks">
        <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0">
              <div className="text-xs font-medium text-[var(--ct-fg)]">Dreaming (Nightly Optimizer)</div>
              <p className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">
                LLM consolidates, dedups, promotes learnings, prunes stale relations. ~3 AM nightly.
              </p>
            </div>
            <button onClick={triggerDream} className="shrink-0 text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80">
              Run Now
            </button>
          </div>
          <Toggle
            label={dreamCron ? "Nightly schedule" : "Loading cron..."}
            checked={dreamCron?.enabled ?? false}
            onChange={(v) => { if (dreamCron) toggleDreamerCron(v); }}
          />
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {renderProviderSelect(
              mem.dreaming_agent,
              (v) => onUpdate("memory", { ...mem, dreaming_agent: v }),
            )}
            {renderModelSelect(
              mem.dreaming_model,
              (v) => onUpdate("memory", { ...mem, dreaming_model: v }),
              dreamingModels,
            )}
          </div>
        </div>

        <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0">
              <div className="text-xs font-medium text-[var(--ct-fg)]">File Sync (Librarian)</div>
              <p className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">
                Re-vectorizes source-tracked files when they change on disk. No LLM.
                {syncStatus && (
                  <> · Tracked: {syncStatus.tracked_files}{syncStatus.stale_files > 0 ? ` (${syncStatus.stale_files} stale)` : ""}</>
                )}
              </p>
            </div>
            <button onClick={triggerSync} disabled={syncing} className="shrink-0 text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80 disabled:opacity-50">
              {syncing ? "Syncing..." : "Sync Now"}
            </button>
          </div>
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
              />
            </Label>
          </div>
        </div>
      </Group>
    </div>
  );
}
