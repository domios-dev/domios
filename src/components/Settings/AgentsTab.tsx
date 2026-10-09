import { useCallback, useEffect } from "react";
import { invoke } from "../../lib/bridge";
import { Bot, ChevronRight, FolderTree, Server, Eye, Timer } from "lucide-react";
import type { AppConfig, OrchestrationConfig, ProviderConfig } from "../../lib/types";
import type { HelpSectionId } from "../../help/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { ProviderInstallCard } from "./ProviderInstallCard";
import { ProviderIntegrationHealthCard } from "./ProviderIntegrationHealthCard";
import { ModelEndpointsCard } from "./model_endpoints/ModelEndpointsCard";
import { providerPageSections, providerSummary } from "./providerPageSections";
import { SupporterAgentCard } from "./SupporterAgentCard";
import { ModelDefaultsCard } from "./ModelDefaultsCard";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import type { SettingsCardAnchorId } from "./sections";
import { SettingsCard, SettingsPill, Toggle } from "./shared";
import { AgentIcon } from "../AgentIcon";

export function AgentsOverview({
  config,
  onUpdate,
  onOpenProvider,
  onOpenGuide,
  focusCard,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenProvider: (providerId: string) => void;
  onOpenGuide: (section: HelpSectionId) => void;
  focusCard?: SettingsCardAnchorId;
}) {
  const { providers, loading, error } = useAgentProviders();
  const authToken = daemonAuthToken(config.api);
  const apiHeaders: HeadersInit = authToken ? { Authorization: `Bearer ${authToken}` } : {};

  return (
    <div className="flex flex-col gap-3" data-testid="settings-agents-overview">
      <SettingsCard icon={<Server size={16} />} title="Providers" description="Installed as plugins from Extensions.">
        {error && <div className="text-[11px] text-[var(--ct-red)]">Failed to load agent providers: {error}</div>}
        {loading && providers.length === 0 && <div className="text-[11px] text-[var(--ct-muted-fg)]">Loading providers…</div>}
        <div className="ct-rows !p-0">
          {providers.map((p) => (
            <button
              key={p.id}
              type="button"
              className="ct-row ct-settings-provider"
              data-testid="settings-agent-provider-row"
              data-provider-id={p.id}
              onClick={() => onOpenProvider(p.id)}
            >
              <span className="ct-settings-card__icon text-[16px]" aria-hidden="true"><AgentIcon providerId={p.id} title={p.display_name} /></span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="ct-settings-nav__label">{p.display_name}</span>
                <span className="ct-settings-nav__desc">{providerSummary(p)}</span>
              </span>
              <ChevronRight size={14} aria-hidden="true" className="ct-settings-nav__chevron" />
            </button>
          ))}
        </div>
      </SettingsCard>

      <SettingsCard
        icon={<Bot size={16} />}
        title="Agent access"
        description="One policy for every provider, applied when an agent starts."
      >
        <Toggle
          label="Full access for agents"
          checked={config.agent?.full_access ?? true}
          onChange={(value) => onUpdate("agent", { full_access: value })}
        />
      </SettingsCard>

      <ModelDefaultsCard config={config} />

      <OrchestrationGroup config={config} onUpdate={onUpdate} />

      <SettingsCardAnchor id="supporter-agent" focused={focusCard === "supporter-agent"}>
        <SupporterAgentCard
          apiBase={daemonHttpBase(config.api)}
          apiHeaders={apiHeaders}
          onOpenGuide={onOpenGuide}
        />
      </SettingsCardAnchor>

      <SettingsCard
        icon={<FolderTree size={16} />}
        title="Skills"
        headerRight={<SettingsPill tone="green">auto-synced</SettingsPill>}
        description={<>Drop skill folders into <code className="ct-detail__mono">~/.codeterm/skills/</code>; every provider sees them.</>}
      >
        <button
          type="button"
          onClick={() => invoke("open_folder", { path: "~/.codeterm/skills" }).catch(() => {})}
          className="ct-bar-pill ct-settings-link"
        >
          Open skills folder
        </button>
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
  const sections = providerPageSections(provider);

  return (
    <div className="flex flex-col gap-5" data-testid="settings-agent-provider-screen" data-provider-id={provider.id}>
      <div className="flex items-center gap-3 pb-2">
        <div className="w-10 h-10 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-[22px] shrink-0">
          <AgentIcon providerId={provider.id} title={provider.display_name} />
        </div>
        <div className="min-w-0">
          <div className="text-xs font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          <div className="text-[11px] text-[var(--ct-muted-fg)]">{providerSummary(provider)}</div>
        </div>
      </div>
      <ProviderInstallCard provider={provider} />
      {sections.integration && <ProviderIntegrationHealthCard provider={provider} />}
      {sections.modelEndpoints && (
        <ModelEndpointsCard providerId={provider.id} harnessName={provider.display_name} daemonApi={fullConfig.api} />
      )}
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
