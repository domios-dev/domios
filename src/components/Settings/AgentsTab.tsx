import { useCallback, useEffect } from "react";
import { invoke } from "../../lib/bridge";
import { Bot, ChevronRight, FolderTree, Server, Eye, Timer } from "lucide-react";
import type { AppConfig, OrchestrationConfig, ProviderConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { ProviderInstallCard } from "./ProviderInstallCard";
import { SettingsCard, SettingsPill, Toggle } from "./shared";
import { AgentIcon } from "../AgentIcon";
import { Button } from "../shared/Button";

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
    <div className="flex flex-col gap-5" data-testid="settings-agents-overview">
      <SettingsCard
        icon={<Bot size={16} />}
        title="Agent access"
        description="One permission policy for every agent provider. Applied whenever an agent starts or restarts."
      >
        <Toggle
          label="Full access for agents"
          checked={config.agent?.full_access ?? true}
          onChange={(value) => onUpdate("agent", { full_access: value })}
        />
      </SettingsCard>

      <OrchestrationGroup config={config} onUpdate={onUpdate} />

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

      <SettingsCard
        icon={<Server size={16} />}
        title="Providers"
        description="Providers are installed and managed as plugins — see the Extensions panel in the titlebar."
      >
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
            <Button
              variant="ghost"
              key={p.id}
              className="h-auto min-h-0 justify-start gap-3 p-4 text-left"
              data-testid="settings-agent-provider-row"
              data-provider-id={p.id}
              onClick={() => onOpenProvider(p.id)}
            >
              <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-[18px] shrink-0">
                <AgentIcon providerId={p.id} title={p.display_name} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-[var(--ct-fg)]">{p.display_name}</div>
                <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">{p.id}</div>
              </div>
              <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" />
            </Button>
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

  // If a plugin was deactivated mid-navigation, pop back to the Agents overview so the user isn't stuck on a dead provider detail.
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
    <div className="flex flex-col gap-5" data-testid="settings-agent-provider-screen" data-provider-id={provider.id}>
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

// Mirrors OrchestrationConfig::default() in codeterm-types config schema.
const ORCHESTRATION_DEFAULTS: OrchestrationConfig = {
  plan_reminders: true,
  nudge_interval_secs: 1800,
};

function NumberRow({
  label,
  value,
  min,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-3" data-testid="settings-number-row">
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <input
        type="number"
        data-testid="settings-number-input"
        value={value}
        min={min}
        onChange={(e) => {
          const n = Number(e.target.value);
          if (Number.isFinite(n)) onChange(Math.max(min, Math.round(n)));
        }}
        className="w-20 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] text-right focus:border-[var(--ct-accent)] focus:outline-none"
      />
    </label>
  );
}

function OrchestrationGroup({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const orch = config.orchestration ?? ORCHESTRATION_DEFAULTS;
  return (
    <SettingsCard
      icon={<Timer size={16} />}
      title="Plan reminders"
      description="Scheduler-owned reminders for agent tabs with active plan work. Reports, sends, polling reads, and idle transitions do not trigger or reset periodic delivery."
    >
      <Toggle
        label="Periodic plan reminders"
        checked={orch.plan_reminders}
        onChange={(v) => onUpdate("orchestration", { plan_reminders: v })}
      />
      {orch.plan_reminders && (
        <>
          <NumberRow
            label="Reminder interval (minutes)"
            value={Math.round(orch.nudge_interval_secs / 60)}
            min={1}
            onChange={(mins) => onUpdate("orchestration", { nudge_interval_secs: mins * 60 })}
          />
        </>
      )}
    </SettingsCard>
  );
}
