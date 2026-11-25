import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Loader } from "../shared/Loader";
import { nativeBridge } from "../../lib/nativeBridge";
import { useCallback, useEffect } from "react";
import { Boxes, ChevronRight, FolderTree, Server, Eye, Sparkles, Timer, Users, Waypoints } from "lucide-react";
import type { AppConfig, OrchestrationConfig, ProviderConfig } from "../../lib/types";
import type { HelpSectionId } from "../../help/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { ProviderInstallCard } from "./ProviderInstallCard";
import { ProviderAccountsSection } from "./ProviderAccountsSection";
import { AcpProfilesCard } from "./AcpProfilesCard";
import { ProviderIntegrationHealthCard } from "./ProviderIntegrationHealthCard";
import { ModelEndpointsCard } from "./model_endpoints/ModelEndpointsCard";
import { providerPageSections, providerSummary } from "./providerPageSections";
import { SupporterAgentCard } from "./SupporterAgentCard";
import { TeamChannelsCard } from "./TeamChannelsCard";
import { ModelDefaultsCard } from "./ModelDefaultsCard";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import type { SettingsCardAnchorId } from "./sections";
import { RemoteLocalOnlyNotice, SettingsCard, SettingsPill, Toggle } from "./shared";
import { useSettingsRemoteTarget } from "./useSettingsTarget";
import { AgentIcon } from "../AgentIcon";
import { NumericInput } from "../shared/NumericInput";

export function AgentsOverview({
  config,
  onUpdate,
  onOpenProvider,
  onOpenGuide,
  focusCard,
  onSwitchGeneralProvider,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenProvider: (providerId: string) => void;
  onOpenGuide: (section: HelpSectionId) => void;
  focusCard?: SettingsCardAnchorId;
  onSwitchGeneralProvider?: (provider: string) => Promise<void>;
}) {
  const remoteTarget = useSettingsRemoteTarget();
  const { providers, loading, error } = useAgentProviders(remoteTarget?.id);
  const authToken = daemonAuthToken(config.api);
  const apiHeaders: HeadersInit = authToken ? { Authorization: `Bearer ${authToken}` } : {};

  return (
    <div className="flex flex-col gap-3" data-testid="settings-agents-overview">
      <SettingsCard icon={<Server size={16} />} title="Harnesses" description="Installed as plugins from Extensions.">
        {error && providers.length === 0 && <InlineErrorNotice error={error} operation="loading harnesses" />}
        {loading && providers.length === 0 && <Loader label="Loading harnesses" />}
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

      <ModelDefaultsCard
        config={config}
        peerId={remoteTarget?.id}
        providers={remoteTarget ? providers : undefined}
        onUpdate={onUpdate}
        onSwitchGeneralProvider={onSwitchGeneralProvider}
        onOpenGuide={onOpenGuide}
      />

      <OrchestrationGroup config={config} onUpdate={onUpdate} />

      <SettingsCardAnchor id="supporter-agent" focused={focusCard === "supporter-agent"}>
        {remoteTarget ? (
          <SettingsCard
            icon={<Sparkles size={16} />}
            title="Spinal Cord"
            description="Helps Domios pick useful memory and read what an agent tab is doing."
          >
            <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="Spinal Cord connections and tokens" />
          </SettingsCard>
        ) : (
          <SupporterAgentCard
            apiBase={daemonHttpBase(config.api)}
            apiHeaders={apiHeaders}
            onOpenGuide={onOpenGuide}
          />
        )}
      </SettingsCardAnchor>

      <SettingsCardAnchor id="team-channels" focused={focusCard === "team-channels"}>
        <TeamChannelsCard />
      </SettingsCardAnchor>

      <SettingsCard
        icon={<FolderTree size={16} />}
        title="Skills"
        headerRight={<SettingsPill tone="green">auto-synced</SettingsPill>}
        description={<>Drop skill folders into <code className="ct-detail__mono">~/.domios/skills/</code>; every harness sees them.</>}
      >
        {remoteTarget && <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="Opening the skills folder" />}
        <button
          type="button"
          disabled={Boolean(remoteTarget)}
          onClick={() => { if (!remoteTarget) void nativeBridge.openFolder("~/.domios/skills").catch(() => {}); }}
          className="ct-bar-pill ct-settings-link disabled:cursor-not-allowed disabled:opacity-50"
        >
          Open skills folder
        </button>
      </SettingsCard>
    </div>
  );
}

export function AgentProviderScreen({
  providerId,
  focusCard,
  fullConfig,
  onUpdateFullConfig,
  onProviderGone,
}: {
  providerId: string;
  focusCard?:SettingsCardAnchorId;
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
  onProviderGone: () => void;
}) {
  const remoteTarget = useSettingsRemoteTarget();
  const { providers, loading, error } = useAgentProviders(remoteTarget?.id);
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

  if (error && !provider) {
    return <InlineErrorNotice error={error} operation="loading harnesses" />;
  }
  if (loading && !provider) {
    return <Loader label="Loading harness" />;
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
      {provider.accounts && (
        <SettingsCardAnchor id="provider-accounts" focused={focusCard==="provider-accounts"}>
          {remoteTarget ? (
            <SettingsCard icon={<Users size={16} />} title="Accounts" description="Tabs start on the first account and move down the list when one reaches its limit.">
              <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="Harness accounts" />
            </SettingsCard>
          ) : (
            <ProviderAccountsSection key={provider.id} provider={provider} />
          )}
        </SettingsCardAnchor>
      )}
      {provider.agent_runtime?.launchProfiles && (
        remoteTarget ? (
          <SettingsCard icon={<Boxes size={16} />} title="Agents" description="The ACP agents Domios can start. Pick one to install or set it up.">
            <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="ACP launch profiles and registry installs" />
          </SettingsCard>
        ) : <AcpProfilesCard provider={provider} />
      )}
      {sections.integration && <ProviderIntegrationHealthCard provider={provider} />}
      {sections.modelEndpoints && (
        remoteTarget ? (
          <SettingsCard
            icon={<Waypoints size={16} />}
            title="Custom model endpoints"
            description={`Add any OpenAI-compatible or Anthropic endpoint; its models appear in ${provider.display_name}'s model picker.`}
          >
            <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="Custom model endpoints and saved keys" />
          </SettingsCard>
        ) : (
          <ModelEndpointsCard providerId={provider.id} harnessName={provider.display_name} daemonApi={fullConfig.api} />
        )
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
        providerDescriptor={remoteTarget ? provider : undefined}
        config={fullConfig}
        onChange={onProviderConfigChange}
        hidePathLinks={Boolean(remoteTarget)}
        disableActions={Boolean(remoteTarget)}
        remoteMachineName={remoteTarget?.name}
      />
    </div>
  );
}

// Mirrors OrchestrationConfig::default() in codeterm-types config schema.
const ORCHESTRATION_DEFAULTS: Pick<OrchestrationConfig, "plan_reminders" | "nudge_interval_secs"> = {
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
      <NumericInput
        data-testid="settings-number-input"
        value={value}
        min={min}
        integer
        onValueChange={onChange}
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
