import type { ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";
import { PairAffordance } from "../shared/PairAffordance";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";

export interface OrchestrationPairValues {
  orchestratorProviderId: string;
  orchestratorModelId: string;
  orchestratorEffortId?: string;
  plannerProviderId: string;
  plannerModelId: string;
  plannerEffortId?: string;
}

interface RoleRowProps {
  label: string;
  providers: ProviderDescriptor[];
  providerId: string;
  modelId: string;
  effortId?: string;
  disabledProviderIds?: ReadonlySet<string>;
  onProviderChange: (id: string) => void;
  onModelChange: (id: string) => void;
  /** Called only when the selected provider declares reasoning efforts. */
  onEffortChange?: (id: string) => void;
  /** Distinct testids for the picker's two role rows (task_234c14f0) — the
   *  shared per-label testid on the row wrapper isn't specific enough for
   *  Playwright to target the orchestrator vs. planner select independently. */
  providerTestId?: string;
  modelTestId?: string;
  effortTestId?: string;
  /** When set, a not-installed provider offers a one-click "Install" that
   *  opens the provider's installer in a terminal tab. */
  onInstallProvider?: (providerId: string) => void;
}

export function OrchestrationRoleRow({
  label,
  providers,
  providerId,
  modelId,
  effortId,
  disabledProviderIds,
  onProviderChange,
  onModelChange,
  onEffortChange,
  providerTestId,
  modelTestId,
  effortTestId,
  onInstallProvider,
}: RoleRowProps) {
  const provider = providers.find((p) => p.id === providerId);
  const models = provider?.available_models ?? [];
  const efforts = provider?.reasoning_efforts ?? [];
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
      ? [{ value: "", label: "No models published", disabled: true }]
      : models.map((m) => ({ value: m.id, label: m.display_name, badge: m.badge ?? undefined }));
  const effortOptions: DropdownSelectOption[] = efforts.map((e) => ({
    value: e.id,
    label: e.display_name,
  }));
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
        {efforts.length > 0 && onEffortChange && (
          <div className="w-[38%] min-w-0">
            <DropdownSelect
              data-testid={effortTestId}
              options={effortOptions}
              value={effortId ?? ""}
              onChange={onEffortChange}
              placeholder="Effort"
              disabled={providerDisabled}
            />
          </div>
        )}
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
    orchestratorEffort?: string;
    plannerProvider?: string;
    plannerModel?: string;
    plannerEffort?: string;
  };
  onInstallProvider?: (providerId: string) => void;
}) {
  const defaultEffortFor = (providerId: string): string | undefined => {
    const provider = providers.find((p) => p.id === providerId);
    const efforts = provider?.reasoning_efforts ?? [];
    if (efforts.length === 0) return undefined;
    return efforts.find((e) => e.id === provider?.default_reasoning_effort)?.id ?? efforts[0]?.id;
  };
  const handleOrchestratorProviderChange = (id: string) => {
    onChange({
      ...values,
      orchestratorProviderId: id,
      orchestratorModelId: providers.find((p) => p.id === id)?.default_model ?? "",
      orchestratorEffortId: defaultEffortFor(id),
    });
  };
  const handlePlannerProviderChange = (id: string) => {
    onChange({
      ...values,
      plannerProviderId: id,
      plannerModelId: providers.find((p) => p.id === id)?.default_model ?? "",
      plannerEffortId: defaultEffortFor(id),
    });
  };

  const orchestratorRow = (
    <OrchestrationRoleRow
      label="Orchestrator"
      providers={providers}
      providerId={values.orchestratorProviderId}
      modelId={values.orchestratorModelId}
      effortId={values.orchestratorEffortId}
      disabledProviderIds={disabledProviderIds}
      onProviderChange={handleOrchestratorProviderChange}
      onModelChange={(modelId) => onChange({ ...values, orchestratorModelId: modelId })}
      onEffortChange={(effortId) => onChange({ ...values, orchestratorEffortId: effortId })}
      providerTestId={roleTestIds?.orchestratorProvider}
      modelTestId={roleTestIds?.orchestratorModel}
      effortTestId={roleTestIds?.orchestratorEffort}
      onInstallProvider={onInstallProvider}
    />
  );
  const plannerRow = (
    <OrchestrationRoleRow
      label="Planner"
      providers={providers}
      providerId={values.plannerProviderId}
      modelId={values.plannerModelId}
      effortId={values.plannerEffortId}
      disabledProviderIds={disabledProviderIds}
      onProviderChange={handlePlannerProviderChange}
      onModelChange={(modelId) => onChange({ ...values, plannerModelId: modelId })}
      onEffortChange={(effortId) => onChange({ ...values, plannerEffortId: effortId })}
      providerTestId={roleTestIds?.plannerProvider}
      modelTestId={roleTestIds?.plannerModel}
      effortTestId={roleTestIds?.plannerEffort}
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
