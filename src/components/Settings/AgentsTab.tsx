import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bell, Bot, ChevronRight, FolderTree, Server, Eye } from "lucide-react";
import type { AppConfig, ProviderConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { ProviderInstallCard } from "./ProviderInstallCard";
import { ProviderNotifications } from "./ProviderNotifications";
import { SettingsCard, SettingsPill, Toggle } from "./shared";
import { AgentIcon } from "../AgentIcon";

export function AgentsOverview({
  config,
  onUpdate,
  onOpenProvider,
  onUpdateFullConfig,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenProvider: (providerId: string) => void;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  const { providers, loading, error } = useAgentProviders();

  return (
    <div className="flex flex-col gap-5">
      <NotificationsGroup
        config={config}
        onUpdate={onUpdate}
        onUpdateFullConfig={onUpdateFullConfig}
      />

      <SettingsCard
        icon={<Bot size={16} />}
        title="General Agent"
        description="Always-on AI assistant in a protected workspace. Starts automatically with CodeTerm. Changes provider on next restart, or click Restart now."
      >
        <Toggle
          label="Enable General Agent"
          checked={config.general_agent?.enabled ?? true}
          onChange={(v) => onUpdate("general_agent", { enabled: v })}
        />
        {config.general_agent?.enabled && (
          <>
            <div className="flex items-center justify-between">
              <span className="text-xs text-[var(--ct-muted-fg)]">Provider</span>
              <div className="flex items-center gap-2">
                <select
                  value={config.general_agent?.agent_provider ?? providers[0]?.id ?? ""}
                  onChange={(e) => onUpdate("general_agent", { agent_provider: e.target.value })}
                  className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                >
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>{p.display_name}</option>
                  ))}
                </select>
                <button
                  onClick={() => {
                    invoke("restart_general_agent").catch((e) => console.error("GA restart failed:", e));
                  }}
                  className="px-2.5 py-1.5 text-[11px] font-medium rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
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
      </SettingsCard>

      <SettingsCard
        icon={<FolderTree size={16} />}
        title="Skills"
        headerRight={<SettingsPill tone="green">auto-synced</SettingsPill>}
        description={
          <>
            Drop skill folders into <code className="text-[10px]">~/.codeterm/skills/</code> — they're
            available to every agent provider automatically.
          </>
        }
      >
        <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
          <span className="font-mono">~/.codeterm/skills/</span>
          <button
            onClick={() => invoke("open_folder", { path: "~/.codeterm/skills" }).catch(() => {})}
            className="text-[var(--ct-accent)] hover:underline"
          >
            Open folder
          </button>
        </div>
      </SettingsCard>

      <SettingsCard icon={<Server size={16} />} title="Providers">
        {error && (
          <div className="text-[10px] text-[var(--ct-red)]">
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
              className="flex items-center gap-3 p-4 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 hover:bg-[var(--ct-hover)] hover:border-[var(--ct-accent)]/50 transition-colors text-left"
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
      </SettingsCard>
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
    <div className="flex flex-col gap-5">
      <div className="flex items-center gap-3 pb-2">
        <div className="w-10 h-10 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-[22px] shrink-0">
          <AgentIcon providerId={provider.id} title={provider.display_name} />
        </div>
        <div className="min-w-0">
          <div className="text-xs font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          <div className="text-[11px] text-[var(--ct-muted-fg)]">{provider.id}</div>
        </div>
      </div>
      <SettingsCard
        icon={<Eye size={16} />}
        title="Visibility"
        description={`When off, the "New ${provider.display_name} Session" entry is hidden from workspace context menus. Spawning via CLI / agent hierarchy still works.`}
      >
        <Toggle
          label="Show in workspace menu"
          checked={visibleInMenu}
          onChange={(v) => onProviderConfigChange(provider.id, { visible_in_menu: v })}
        />
      </SettingsCard>
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

function NotificationsGroup({
  config,
  onUpdate,
  onUpdateFullConfig,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  return (
    <SettingsCard
      icon={<Bell size={16} />}
      title="Notifications"
      description="Get alerted when an agent finishes a task or needs your attention. Hooks auto-install on startup."
    >
      <Toggle
        label="Desktop notifications"
        checked={config.notifications.desktop}
        onChange={(v) => onUpdate("notifications", { desktop: v })}
      />
      <Toggle
        label="Sound"
        checked={config.notifications.sound}
        onChange={(v) => onUpdate("notifications", { sound: v })}
      />
      <label className="flex items-center justify-between opacity-50 cursor-not-allowed">
        <span className="text-xs text-[var(--ct-muted-fg)]">In-app toast (coming soon)</span>
        <span
          className="relative inline-flex h-5 w-9 items-center rounded-full bg-[var(--ct-border)]"
        >
          <span className="inline-block h-3.5 w-3.5 rounded-full bg-white translate-x-0.5" />
        </span>
      </label>

      <ProviderNotifications
        fullConfig={config}
        onUpdateFullConfig={onUpdateFullConfig}
        hideActions
        hidePathLinks
      />
    </SettingsCard>
  );
}
