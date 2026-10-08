import { Cpu, RefreshCcw } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { modelDefaultRows } from "./modelDefaultsModel";
import { SettingsCard } from "./shared";

export function ModelDefaultsCard({ config }: { config: AppConfig }) {
  const descriptors = useAgentDescriptors();
  const rows = modelDefaultRows(config);
  const providerName = (id: string) => descriptors.find((provider) => provider.id === id)?.display_name ?? id;
  return (
    <SettingsCard
      icon={<Cpu size={16} />}
      title="Model defaults"
      description="Models used when nothing else picks one. When a provider stops offering a model, its default moves to the closest model it still offers."
    >
      {rows.length === 0 ? (
        <p className="text-xs text-[var(--ct-muted-fg)]">
          No model defaults are set, so each provider starts sessions on its own default.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-[var(--ct-border-subtle)]" aria-label="Model defaults">
          {rows.map((row) => (
            <li key={row.key} className="flex items-baseline justify-between gap-3 py-2" data-testid="model-default-row">
              <span className="min-w-0 text-xs text-[var(--ct-fg)]">
                {row.subject}
                <span className="text-[var(--ct-muted-fg)]"> for {providerName(row.provider)}</span>
              </span>
              <span className="flex min-w-0 flex-col items-end gap-0.5 text-right">
                <code className="truncate text-[11px] text-[var(--ct-fg)]">{row.model}</code>
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
