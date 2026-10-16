import { Cpu, RefreshCcw } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { modelDefaultGuideSection, modelDefaultPatch, modelDefaultRows } from "./modelDefaultsModel";
import { GeneralAgentProviderChoice } from "./GeneralAgentProviderChoice";
import { DefaultModelPicker } from "./DefaultModelPicker";
import { SettingsCard } from "./shared";
import type { HelpSectionId } from "../../help/types";
import { GuideInfoButton } from "./GuideInfoButton";

export function ModelDefaultsCard({ config, onUpdate, onSwitchGeneralProvider, onOpenGuide }: {
  config: AppConfig;
  onSwitchGeneralProvider?: (provider: string) => Promise<void>;
  onOpenGuide?: (section: HelpSectionId) => void;
  onUpdate?: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const descriptors = useAgentDescriptors();
  const rows = modelDefaultRows(config);
  const providerName = (id: string) => descriptors.find((provider) => provider.id === id)?.display_name ?? id;
  return (
    <SettingsCard
      icon={<Cpu size={16} />}
      title="Model defaults"
      description="Models used when nothing else picks one. When a provider stops offering a model, its default moves to the closest model it still offers."
    >
      {onSwitchGeneralProvider && <div className="mb-2 border-b border-[var(--ct-border)] pb-5">
        <GeneralAgentProviderChoice current={config.general_agent?.agent_provider} onOpenGuide={onOpenGuide}
          enabled={config.general_agent?.enabled ?? true} onSwitch={onSwitchGeneralProvider} />
      </div>}
      {rows.some((row) => row.key.startsWith("memory.")) && <h3 className="mt-2 text-sm font-medium text-[var(--ct-fg)]">Background models</h3>}
      {rows.length === 0 ? (
        <p className="text-xs text-[var(--ct-muted-fg)]">
          No model defaults are set, so each provider starts sessions on its own default.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-[var(--ct-border-subtle)]" aria-label="Model defaults">
          {rows.map((row) => (
            <li key={row.key} className="flex items-baseline justify-between gap-3 py-2" data-testid="model-default-row">
              <span className="flex min-w-0 items-center gap-1 text-xs text-[var(--ct-fg)]">
                <span>
                {row.subject}
                <span className="text-[var(--ct-muted-fg)]"> for {providerName(row.provider)}</span>
                </span>
                {onOpenGuide && <GuideInfoButton label={row.subject} section={modelDefaultGuideSection(row)} onOpen={onOpenGuide} />}
              </span>
              <span className="flex min-w-0 flex-col items-end gap-0.5 text-right">
                {onUpdate ? <DefaultModelPicker providerId={row.provider} value={row.model} onChange={(model) => {
                  const patch = modelDefaultPatch(config, row, model);
                  if (patch.agents) onUpdate("agents", patch.agents);
                  if (patch.memory) onUpdate("memory", patch.memory);
                }} /> : <code className="truncate text-[11px] text-[var(--ct-fg)]">{row.model}</code>}
                {row.replacedFrom && (
                  <span className="inline-flex items-center gap-1 text-[10.5px] text-[var(--ct-accent)]" data-testid="model-default-replaced">
                    <RefreshCcw size={10} aria-hidden />
                    Updated automatically from <code>{row.replacedFrom}</code>
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </SettingsCard>
  );
}
