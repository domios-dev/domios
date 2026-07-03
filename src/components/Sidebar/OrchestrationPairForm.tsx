import type { ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";
import { PairAffordance } from "../shared/PairAffordance";

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
}

export function OrchestrationRoleRow({
  label,
  providers,
  providerId,
  modelId,
  disabledProviderIds,
  onProviderChange,
  onModelChange,
}: RoleRowProps) {
  const provider = providers.find((p) => p.id === providerId);
  const models = provider?.available_models ?? [];
  const providerDisabled = disabledProviderIds?.has(providerId) === true;
  return (
    <div className="orchestration-role-row flex flex-col gap-1.5" data-testid={`orchestration-role-${label.toLowerCase()}`}>
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
      <div className="flex items-center gap-2">
        <div className="flex items-center gap-1.5 flex-1 min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)]">
          <AgentIcon providerId={providerId} className="text-[12px] shrink-0" />
          <select
            value={providerId}
            onChange={(e) => onProviderChange(e.target.value)}
            className="flex-1 min-w-0 bg-transparent text-[12px] text-[var(--ct-fg)] outline-none"
          >
            {providers.map((p) => {
              const disabled = disabledProviderIds?.has(p.id) === true;
              return (
                <option key={p.id} value={p.id} disabled={disabled}>
                  {p.display_name}
                  {disabled ? " (not installed)" : ""}
                </option>
              );
            })}
          </select>
        </div>
        <select
          value={modelId}
          onChange={(e) => onModelChange(e.target.value)}
          disabled={models.length === 0 || providerDisabled}
          className="flex-1 min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[12px] text-[var(--ct-fg)] outline-none disabled:opacity-40"
        >
          {models.length === 0 ? (
            <option value="">default</option>
          ) : (
            models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.display_name}
              </option>
            ))
          )}
        </select>
      </div>
      {providerDisabled && (
        <div className="text-[10px] text-[var(--ct-muted-fg)]">CLI not installed for this provider</div>
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
}: {
  providers: ProviderDescriptor[];
  values: OrchestrationPairValues;
  disabledProviderIds?: ReadonlySet<string>;
  onChange: (next: OrchestrationPairValues) => void;
  layout?: "card" | "stack";
  titleField?: ReactNode;
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
        data-testid="new-session-pair-affordance"
        orchestrator={orchestratorRow}
        planner={plannerRow}
      />
      {titleField}
    </div>
  );
}
