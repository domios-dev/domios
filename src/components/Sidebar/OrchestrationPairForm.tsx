import type { ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";
import { PairAffordance } from "../shared/PairAffordance";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";

export interface OrchestrationPairValues {
  orchestratorProviderId: string;
  orchestratorModelId: string;
  plannerProviderId: string;
  plannerModelId: string;
}

interface RoleRowProps {
  label: string;
  providers: ProviderDescriptor[];
  providerId: string;
  modelId: string;
  disabledProviderIds?: ReadonlySet<string>;
  onProviderChange: (id: string) => void;
  onModelChange: (id: string) => void;
  /** Distinct testids for the picker's two role rows (task_234c14f0) — the
   *  shared per-label testid on the row wrapper isn't specific enough for
   *  Playwright to target the orchestrator vs. planner select independently. */
  providerTestId?: string;
  modelTestId?: string;
  /** When set, a not-installed provider offers a one-click "Install" that
   *  opens the provider's installer in a terminal tab. */
  onInstallProvider?: (providerId: string) => void;
}

export function OrchestrationRoleRow({
  label,
  providers,
  providerId,
  modelId,
  disabledProviderIds,
  onProviderChange,
  onModelChange,
  providerTestId,
  modelTestId,
  onInstallProvider,
}: RoleRowProps) {
  const provider = providers.find((p) => p.id === providerId);
  const models = provider?.available_models ?? [];
  const providerDisabled = disabledProviderIds?.has(providerId) === true;
  const providerOptions: DropdownSelectOption[] = providers.map((p) => {
    const disabled = disabledProviderIds?.has(p.id) === true;
    return {
      value: p.id,
      label: p.display_name,
      disabled,
      suffix: disabled ? " (not installed)" : undefined,
    };
  });
  const modelOptions: DropdownSelectOption[] =
    models.length === 0
      ? [{ value: "", label: "default" }]
      : models.map((m) => ({ value: m.id, label: m.display_name }));
  return (
    <div className="orchestration-role-row flex flex-col gap-1.5" data-testid={`orchestration-role-${label.toLowerCase()}`}>
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
      <div className="flex items-center gap-2">
        <div className="flex-1 min-w-0">
          <DropdownSelect
            data-testid={providerTestId}
            options={providerOptions}
            value={providerId}
            onChange={onProviderChange}
            leading={<AgentIcon providerId={providerId} className="text-[12px] shrink-0" />}
          />
        </div>
        <div className="flex-1 min-w-0">
          <DropdownSelect
            data-testid={modelTestId}
            options={modelOptions}
            value={modelId}
            onChange={onModelChange}
            disabled={models.length === 0 || providerDisabled}
          />
        </div>
      </div>
      {providerDisabled && (
        <div className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)]">
          <span>{provider?.display_name ?? "This provider"}'s CLI isn't installed</span>
          {onInstallProvider && provider?.install_command && (
            <button
              type="button"
              data-testid={`install-provider-${providerId}`}
              onClick={() => onInstallProvider(providerId)}
              className="rounded border px-1.5 py-0.5 text-[10px] font-medium text-[var(--ct-accent)] hover:brightness-125"
              style={{ borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 40%, transparent)" }}
            >
              Install in terminal
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export function OrchestrationPairForm({
  providers,
  values,
  disabledProviderIds,
  onChange,
  layout = "card",
  titleField,
  pairAffordanceTestId,
  roleTestIds,
  onInstallProvider,
}: {
  providers: ProviderDescriptor[];
  values: OrchestrationPairValues;
  disabledProviderIds?: ReadonlySet<string>;
  onChange: (next: OrchestrationPairValues) => void;
  layout?: "card" | "stack";
  titleField?: ReactNode;
  pairAffordanceTestId?: string;
  roleTestIds?: {
    orchestratorProvider?: string;
    orchestratorModel?: string;
    plannerProvider?: string;
    plannerModel?: string;
  };
  onInstallProvider?: (providerId: string) => void;
}) {
  const handleOrchestratorProviderChange = (id: string) => {
    onChange({
      ...values,
      orchestratorProviderId: id,
      orchestratorModelId: providers.find((p) => p.id === id)?.default_model ?? "",
    });
  };
  const handlePlannerProviderChange = (id: string) => {
    onChange({
      ...values,
      plannerProviderId: id,
      plannerModelId: providers.find((p) => p.id === id)?.default_model ?? "",
    });
  };

  const orchestratorRow = (
    <OrchestrationRoleRow
      label="Orchestrator"
      providers={providers}
      providerId={values.orchestratorProviderId}
      modelId={values.orchestratorModelId}
      disabledProviderIds={disabledProviderIds}
      onProviderChange={handleOrchestratorProviderChange}
      onModelChange={(modelId) => onChange({ ...values, orchestratorModelId: modelId })}
      providerTestId={roleTestIds?.orchestratorProvider}
      modelTestId={roleTestIds?.orchestratorModel}
      onInstallProvider={onInstallProvider}
    />
  );
  const plannerRow = (
    <OrchestrationRoleRow
      label="Planner"
      providers={providers}
      providerId={values.plannerProviderId}
      modelId={values.plannerModelId}
      disabledProviderIds={disabledProviderIds}
      onProviderChange={handlePlannerProviderChange}
      onModelChange={(modelId) => onChange({ ...values, plannerModelId: modelId })}
      providerTestId={roleTestIds?.plannerProvider}
      modelTestId={roleTestIds?.plannerModel}
      onInstallProvider={onInstallProvider}
    />
  );

  if (layout === "stack") {
    return (
      <div className="flex flex-col gap-3">
        {orchestratorRow}
        {plannerRow}
        {titleField}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <PairAffordance
        layout="card"
        data-testid={pairAffordanceTestId ?? "new-session-pair-affordance"}
        orchestrator={orchestratorRow}
        planner={plannerRow}
      />
      {titleField}
    </div>
  );
}
