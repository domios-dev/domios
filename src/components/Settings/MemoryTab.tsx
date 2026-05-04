import { useCallback, useEffect, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { Group, Label } from "./shared";

export function MemoryTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const defaultMem = { extraction_agent: "", extraction_model: "haiku", auto_extract: true, dreaming_enabled: true, dreaming_agent: "", dreaming_model: "haiku", worker_model: "claude-haiku-4-5-20251001", worker_timeout_seconds: 150, file_sync_enabled: true, file_sync_interval_minutes: 60, dedup_threshold: 0.15 };
  const mem = { ...defaultMem, ...(config.memory ?? {}) };
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

  useEffect(() => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/mem/sync/status`, { headers: apiHeaders })
      .then(r => r.json())
      .then(d => setSyncStatus(d))
      .catch(() => {});
  }, [apiBase]);

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

  return (
    <div className="space-y-6">
      <Group title="Extraction Workers">
        <p className="text-[11px] text-[var(--ct-muted)] mb-3">
          When you save memories, extraction workers analyze the content and build a knowledge graph — entities, relations, decisions, problems. Workers run as headless agents in the Memory Panel.
        </p>
        <Label text="Auto-extraction">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={mem.auto_extract}
              onChange={(e) => onUpdate("memory", { ...mem, auto_extract: e.target.checked })}
              className="w-4 h-4 rounded"
            />
            <span className="text-xs text-[var(--ct-fg)]">Spawn extraction workers on save</span>
          </label>
        </Label>
        <Label text="Agent provider">
          <select
            value={mem.extraction_agent || ""}
            onChange={(e) => onUpdate("memory", { ...mem, extraction_agent: e.target.value })}
            className={inputClass}
          >
            <option value="">Default ({config.general_agent?.agent_provider || "claude"})</option>
            {providers.map(p => (
              <option key={p.id} value={p.id}>{p.display_name}</option>
            ))}
          </select>
        </Label>
        <Label text="Model">
          {extractionModels.length > 0 ? (
            <select
              value={mem.extraction_model}
              onChange={(e) => onUpdate("memory", { ...mem, extraction_model: e.target.value })}
              className={inputClass}
            >
              <option value="">Provider default</option>
              {extractionModels.map(m => (
                <option key={m.id} value={m.id}>{m.display_name}</option>
              ))}
            </select>
          ) : (
            <input
              value={mem.extraction_model}
              onChange={(e) => onUpdate("memory", { ...mem, extraction_model: e.target.value })}
              className={inputClass}
              placeholder="haiku, sonnet, o3-mini..."
            />
          )}
        </Label>
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
          <p className="text-[10px] text-[var(--ct-muted)] mt-1">
            Cosine distance below this triggers dedup investigation. Lower = stricter. Default 0.15.
          </p>
        </Label>
        <Label text="Run timeout (seconds)">
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
      </Group>

      <Group title="Periodic Tasks">
        <div className="space-y-4">
          <div className="border border-[var(--ct-border)] rounded p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-[var(--ct-fg)]">Dreaming (Nightly Optimizer)</span>
              <button onClick={triggerDream} className="text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80">Run Now</button>
            </div>
            <p className="text-[10px] text-[var(--ct-muted)] mb-2">
              Uses LLM — consolidates, deduplicates, promotes learnings, cleans stale graph relations. Schedule: ~3 AM with jitter.
            </p>
            <Label text="Enable">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={mem.dreaming_enabled}
                  onChange={(e) => onUpdate("memory", { ...mem, dreaming_enabled: e.target.checked })}
                  className="w-4 h-4 rounded"
                />
                <span className="text-xs text-[var(--ct-fg)]">Nightly memory optimization</span>
              </label>
            </Label>
            <Label text="Agent provider">
              <select
                value={mem.dreaming_agent || ""}
                onChange={(e) => onUpdate("memory", { ...mem, dreaming_agent: e.target.value })}
                className={inputClass}
              >
                <option value="">Default ({defaultProvider})</option>
                {providers.map(p => (
                  <option key={p.id} value={p.id}>{p.display_name}</option>
                ))}
              </select>
            </Label>
            <Label text="Model">
              {dreamingModels.length > 0 ? (
                <select
                  value={mem.dreaming_model}
                  onChange={(e) => onUpdate("memory", { ...mem, dreaming_model: e.target.value })}
                  className={inputClass}
                >
                  <option value="">Provider default</option>
                  {dreamingModels.map(m => (
                    <option key={m.id} value={m.id}>{m.display_name}</option>
                  ))}
                </select>
              ) : (
                <input
                  value={mem.dreaming_model}
                  onChange={(e) => onUpdate("memory", { ...mem, dreaming_model: e.target.value })}
                  className={inputClass}
                  placeholder="haiku, sonnet, o3-mini..."
                />
              )}
            </Label>
          </div>

          <div className="border border-[var(--ct-border)] rounded p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-[var(--ct-fg)]">File Sync (Librarian)</span>
              <button onClick={triggerSync} disabled={syncing} className="text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80 disabled:opacity-50">
                {syncing ? "Syncing..." : "Sync Now"}
              </button>
            </div>
            <p className="text-[10px] text-[var(--ct-muted)] mb-2">
              No LLM — pure Rust mtime comparison + re-vectorization for source-tracked files.
            </p>
            {syncStatus && (
              <p className="text-[10px] text-[var(--ct-muted)] mb-2">
                Tracked files: {syncStatus.tracked_files}{syncStatus.stale_files > 0 ? ` (${syncStatus.stale_files} stale)` : ""}
              </p>
            )}
            <Label text="Enable">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={mem.file_sync_enabled}
                  onChange={(e) => onUpdate("memory", { ...mem, file_sync_enabled: e.target.checked })}
                  className="w-4 h-4 rounded"
                />
                <span className="text-xs text-[var(--ct-fg)]">Periodic file sync</span>
              </label>
            </Label>
            <Label text="Sync interval (minutes)">
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
