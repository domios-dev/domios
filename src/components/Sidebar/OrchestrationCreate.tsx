/**
 * Paired creation (task_7e5f9354): modal wrapper around OrchestrationPairForm.
 * Provider/model options come from descriptors — no hardcoded model ids.
 */
import { useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { OrchestrationPairForm } from "./OrchestrationPairForm";

export interface OrchestrationCreateResult {
  orchestratorAgent: string;
  orchestratorModel?: string;
  plannerAgent: string;
  plannerModel?: string;
  title?: string;
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
  const [title, setTitle] = useState("");

  const canConfirm = pairValues.orchestratorProviderId.length > 0 && pairValues.plannerProviderId.length > 0;

  const handleConfirm = () => {
    if (!canConfirm) return;
    onConfirm({
      orchestratorAgent: pairValues.orchestratorProviderId,
      orchestratorModel: pairValues.orchestratorModelId || undefined,
      plannerAgent: pairValues.plannerProviderId,
      plannerModel: pairValues.plannerModelId || undefined,
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
        <div className="px-4 py-3">
          <OrchestrationPairForm
            providers={providers}
            values={pairValues}
            onChange={setPairValues}
            layout="stack"
            titleField={(
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
            )}
          />
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
