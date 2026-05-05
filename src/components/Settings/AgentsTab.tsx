import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronRight } from "lucide-react";
import type { AppConfig, ProviderConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { ProviderInstallCard } from "./ProviderInstallCard";
import { Toggle } from "./shared";
import { AgentIcon } from "../AgentIcon";

export function AgentsOverview({
  config,
  onUpdate,
  onOpenProvider,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenProvider: (providerId: string) => void;
}) {
  const { providers, loading, error } = useAgentProviders();

  return (
    <div className="space-y-5">
      {/* General Agent — shared across all providers */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="text-sm font-medium text-[var(--ct-fg)]">General Agent</div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Always-on AI assistant in a protected workspace. Starts automatically with CodeTerm.
        </div>
        <Toggle
          label="Enable General Agent"
          checked={config.general_agent?.enabled ?? true}
          onChange={(v) => onUpdate("general_agent", { enabled: v })}
        />
        {config.general_agent?.enabled && (
          <>
            <div className="flex items-center justify-between">
              <span className="text-xs text-[var(--ct-fg)]">Provider</span>
              <div className="flex items-center gap-2">
                <select
                  value={config.general_agent?.agent_provider ?? "claude"}
                  onChange={(e) => onUpdate("general_agent", { agent_provider: e.target.value })}
                  className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                >
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>{p.display_name}</option>
                  ))}
                </select>
                <button
                  onClick={() => {
                    invoke("restart_general_agent").catch((e) => console.error("GA restart failed:", e));
                  }}
                  className="px-2.5 py-1 text-[10px] font-medium rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
                >
                  Restart
                </button>
              </div>
            </div>
            <Toggle
              label="Auto-restart on crash"
              checked={config.general_agent?.auto_restart ?? true}
              onChange={(v) => onUpdate("general_agent", { auto_restart: v })}
            />
          </>
        )}
        <div className="text-[10px] text-[var(--ct-muted)]">
          Changes provider on next restart, or click Restart now.
        </div>
      </div>

      {/* Skills — CodeTerm-wide source of truth, NOT a provider contribution.
          Skills are a CodeTerm feature that syncs to all providers. */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Skills</div>
          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">
            auto-synced
          </span>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Drop skill folders into <code className="text-[10px]">~/.codeterm/skills/</code> — they're
          available to every agent provider automatically.
        </div>
        <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
          <span className="font-mono">~/.codeterm/skills/</span>
          <button
            onClick={() => invoke("open_folder", { path: "~/.codeterm/skills" }).catch(() => {})}
            className="text-[var(--ct-accent)] hover:underline"
          >
            Open folder
          </button>
        </div>
      </div>

      {/* Providers — tile list, click to open detail screen */}
      <div>
        <div className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)] mb-2.5">Providers</div>
        {error && (
          <div className="text-[10px] text-[var(--ct-red)] mb-3">
            Failed to load agent providers: {error}
          </div>
        )}
        {loading && providers.length === 0 && (
          <div className="text-[10px] text-[var(--ct-muted-fg)]">Loading providers…</div>
        )}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {providers.map((p) => (
            <button
              key={p.id}
              onClick={() => onOpenProvider(p.id)}
              className="flex items-center gap-3 p-3.5 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 hover:bg-[var(--ct-hover)] hover:border-[var(--ct-accent)]/50 transition-colors text-left"
            >
              <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-[18px] shrink-0">
                <AgentIcon providerId={p.id} title={p.display_name} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-[var(--ct-fg)]">{p.display_name}</div>
                <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">{p.id}</div>
              </div>
              <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" />
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export function AgentProviderScreen({
  providerId,
  fullConfig,
  onUpdateFullConfig,
  onProviderGone,
}: {
  providerId: string;
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
  onProviderGone: () => void;
}) {
  const { providers, loading, error } = useAgentProviders();
  const provider = providers.find((p) => p.id === providerId) ?? null;

  // If a plugin was deactivated mid-navigation, pop back to the Agents
  // overview so the user isn't stuck on a dead provider detail.
  useEffect(() => {
    if (!loading && !provider) onProviderGone();
  }, [loading, provider, onProviderGone]);

  const onProviderConfigChange = useCallback(
    (pid: string, patch: Partial<ProviderConfig>) => {
      const prev = fullConfig.agents?.[pid] ?? {};
      const nextProvider: ProviderConfig = { ...prev, ...patch };
      const nextAgents = { ...(fullConfig.agents ?? {}), [pid]: nextProvider };
      onUpdateFullConfig({ ...fullConfig, agents: nextAgents });
    },
    [fullConfig, onUpdateFullConfig],
  );

  if (error) {
    return <div className="text-[10px] text-[var(--ct-red)]">Failed to load agent providers: {error}</div>;
  }
  if (loading && !provider) {
    return <div className="text-[10px] text-[var(--ct-muted-fg)]">Loading provider…</div>;
  }
  if (!provider) {
    // Transient: the pop effect above will unmount us on the next tick.
    return null;
  }

  const providerCfg = fullConfig.agents?.[provider.id] ?? {};
  const visibleInMenu = providerCfg.visible_in_menu !== false;

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3 pb-2">
        <div className="w-10 h-10 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-[22px] shrink-0">
          <AgentIcon providerId={provider.id} title={provider.display_name} />
        </div>
        <div className="min-w-0">
          <div className="text-sm font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          <div className="text-[11px] text-[var(--ct-muted-fg)]">{provider.id}</div>
        </div>
      </div>
      {/* CodeTerm-wide policy: hide this provider from the per-workspace
          'New ... Session' menu. Lives outside the plugin schema because
          it's a host-side filter, not a per-plugin setting. */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4">
        <Toggle
          label="Show in workspace menu"
          checked={visibleInMenu}
          onChange={(v) => onProviderConfigChange(provider.id, { visible_in_menu: v })}
        />
        <div className="text-[10px] text-[var(--ct-muted)] mt-1">
          When off, the "New {provider.display_name} Session" entry is hidden from workspace context menus. Spawning via CLI / agent hierarchy still works.
        </div>
      </div>
      <ProviderInstallCard provider={provider} />
      <SchemaRenderer
        schema={provider.settings_schema}
        providerId={provider.id}
        config={fullConfig}
        onChange={onProviderConfigChange}
      />
    </div>
  );
}
