/**
 * Paired creation (task_7e5f9354): "New session" → "Orchestration" opens this
 * form to spawn an Orchestrator + Planner pair bound to a fresh plan in one
 * action. Provider/model options come straight from the provider descriptors
 * (`available_models` / `default_model`) — no hardcoded model ids.
 */
import { useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";

export interface OrchestrationCreateResult {
  orchestratorAgent: string;
  orchestratorModel?: string;
  plannerAgent: string;
  plannerModel?: string;
  title?: string;
}

interface RoleRowProps {
  label: string;
  providers: ProviderDescriptor[];
  providerId: string;
  modelId: string;
  onProviderChange: (id: string) => void;
  onModelChange: (id: string) => void;
}

function RoleRow({ label, providers, providerId, modelId, onProviderChange, onModelChange }: RoleRowProps) {
  const provider = providers.find((p) => p.id === providerId);
  const models = provider?.available_models ?? [];
  return (
    <div className="flex flex-col gap-1.5">
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
      <div className="flex items-center gap-2">
        <div className="flex items-center gap-1.5 flex-1 min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)]">
          <AgentIcon providerId={providerId} className="text-[12px] shrink-0" />
          <select
            value={providerId}
            onChange={(e) => onProviderChange(e.target.value)}
            className="flex-1 min-w-0 bg-transparent text-[12px] text-[var(--ct-fg)] outline-none"
          >
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.display_name}
              </option>
            ))}
          </select>
        </div>
        <select
          value={modelId}
          onChange={(e) => onModelChange(e.target.value)}
          disabled={models.length === 0}
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
    </div>
  );
}

export function OrchestrationCreate({
  providers,
  onCancel,
  onConfirm,
}: {
  providers: ProviderDescriptor[];
  onCancel: () => void;
  onConfirm: (result: OrchestrationCreateResult) => void;
}) {
  const defaultProviderId = providers[0]?.id ?? "";
  const [orchestratorProviderId, setOrchestratorProviderId] = useState(defaultProviderId);
  const [plannerProviderId, setPlannerProviderId] = useState(defaultProviderId);
  const [orchestratorModelId, setOrchestratorModelId] = useState(
    providers[0]?.default_model ?? "",
  );
  const [plannerModelId, setPlannerModelId] = useState(providers[0]?.default_model ?? "");
  const [title, setTitle] = useState("");

  const handleOrchestratorProviderChange = (id: string) => {
    setOrchestratorProviderId(id);
    setOrchestratorModelId(providers.find((p) => p.id === id)?.default_model ?? "");
  };
  const handlePlannerProviderChange = (id: string) => {
    setPlannerProviderId(id);
    setPlannerModelId(providers.find((p) => p.id === id)?.default_model ?? "");
  };

  const canConfirm = orchestratorProviderId.length > 0 && plannerProviderId.length > 0;

  const handleConfirm = () => {
    if (!canConfirm) return;
    onConfirm({
      orchestratorAgent: orchestratorProviderId,
      orchestratorModel: orchestratorModelId || undefined,
      plannerAgent: plannerProviderId,
      plannerModel: plannerModelId || undefined,
      title: title.trim() || undefined,
    });
  };

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onCancel} />
      <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[360px] overflow-hidden">
        <div className="px-4 pt-4 pb-2">
          <div className="text-[13px] font-medium text-[var(--ct-fg)]">New Orchestration</div>
          <div className="mt-1 text-[11px] text-[var(--ct-muted)]">
            Spawns an Orchestrator + Planner pair, linked from birth via a shared plan.
          </div>
        </div>
        <div className="px-4 py-3 flex flex-col gap-3">
          <RoleRow
            label="Orchestrator"
            providers={providers}
            providerId={orchestratorProviderId}
            modelId={orchestratorModelId}
            onProviderChange={handleOrchestratorProviderChange}
            onModelChange={setOrchestratorModelId}
          />
          <RoleRow
            label="Planner"
            providers={providers}
            providerId={plannerProviderId}
            modelId={plannerModelId}
            onProviderChange={handlePlannerProviderChange}
            onModelChange={setPlannerModelId}
          />
          <div className="flex flex-col gap-1.5">
            <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Plan title (optional)</div>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Orchestration"
              className="px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[12px] text-[var(--ct-fg)] outline-none"
            />
          </div>
        </div>
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleConfirm}
            disabled={!canConfirm}
            className="px-4 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 disabled:opacity-40 transition-opacity"
          >
            Create
          </button>
        </div>
      </div>
    </>
  );
}
