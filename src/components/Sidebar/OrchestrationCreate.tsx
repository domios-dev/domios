import { useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { OrchestrationPairForm } from "./OrchestrationPairForm";
import { Button } from "../shared/Button";

export interface OrchestrationCreateResult {
  orchestratorAgent: string;
  orchestratorModel?: string;
  plannerAgent: string;
  plannerModel?: string;
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
  const [pairValues, setPairValues] = useState({
    orchestratorProviderId: defaultProviderId,
    orchestratorModelId: providers[0]?.default_model ?? "",
    plannerProviderId: defaultProviderId,
    plannerModelId: providers[0]?.default_model ?? "",
  });
  const canConfirm = pairValues.orchestratorProviderId.length > 0 && pairValues.plannerProviderId.length > 0;

  const handleConfirm = () => {
    if (!canConfirm) return;
    onConfirm({
      orchestratorAgent: pairValues.orchestratorProviderId,
      orchestratorModel: pairValues.orchestratorModelId || undefined,
      plannerAgent: pairValues.plannerProviderId,
      plannerModel: pairValues.plannerModelId || undefined,
    });
  };

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onCancel} />
      <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[360px] overflow-hidden">
        <div className="px-4 pt-4 pb-2">
          <div className="text-[13px] font-medium text-[var(--ct-fg)]">New Orchestration</div>
          <div className="mt-1 text-[11px] text-[var(--ct-muted)]">
            Starts an Orchestrator and Planner pair. They wait for your first goal before creating a plan.
          </div>
        </div>
        <div className="px-4 py-3">
          <OrchestrationPairForm
            providers={providers}
            values={pairValues}
            onChange={setPairValues}
            layout="stack"
            roleTestIds={{
              orchestratorProvider: "orchestration-create-orchestrator-provider",
              orchestratorModel: "orchestration-create-orchestrator-model",
              plannerProvider: "orchestration-create-planner-provider",
              plannerModel: "orchestration-create-planner-model",
            }}
          />
        </div>
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
          <Button variant="ghost" size="sm" onClick={onCancel}>Cancel</Button>
          <Button variant="accent" size="sm" onClick={handleConfirm} disabled={!canConfirm}>Create</Button>
        </div>
      </div>
    </>
  );
}
