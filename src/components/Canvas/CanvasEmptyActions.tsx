import { useCallback, useState } from "react";
import { Plus } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { useCanvasWorkspaceTarget, type CanvasWorkspaceTarget } from "./canvasFocusStore";
import { spawnOrchestrator } from "./canvasApi";
import { createPlan as createPlanRequest, type CreatePlanParams } from "./plansApi";

export interface CanvasEmptyActionsProps {
  api: ApiConfig;
  projectPath?: string | null;
}

interface CreateCanvasPlanArgs {
  api: ApiConfig;
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  spawnOrchestrator: (api: ApiConfig) => Promise<string | null>;
  createPlan: (api: ApiConfig, params: CreatePlanParams) => Promise<unknown>;
}

export async function createCanvasPlan({
  api,
  projectPath,
  workspaceTarget,
  spawnOrchestrator: spawn,
  createPlan,
}: CreateCanvasPlanArgs): Promise<void> {
  const isTeamTarget = workspaceTarget?.teamId != null;
  const paneId = isTeamTarget
    ? workspaceTarget?.managerPaneId ?? null
    : await spawn(api);
  await createPlan(api, {
    title: "New plan",
    project_path: projectPath ?? undefined,
    owner: paneId
      ? {
        pane_id: paneId,
        session_id: null,
        title: isTeamTarget ? "Team Manager" : "Canvas Orchestrator",
      }
      : undefined,
  });
}

export function CanvasEmptyActions({ api, projectPath }: CanvasEmptyActionsProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const workspaceTarget = useCanvasWorkspaceTarget();

  const onCreate = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await createCanvasPlan({
        api,
        projectPath,
        workspaceTarget,
        spawnOrchestrator,
        createPlan: createPlanRequest,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [api, busy, projectPath, workspaceTarget]);

  return (
    <div className="flex flex-col items-center gap-3">
      <button
        type="button"
        data-testid="canvas-create-plan"
        disabled={busy}
        onClick={() => void onCreate()}
        className="inline-flex items-center gap-2 rounded border px-4 py-2 text-sm cursor-pointer disabled:opacity-50"
        style={{
          background: "var(--ct-bg)",
          borderColor: "var(--ct-border)",
          color: "var(--ct-fg)",
        }}
      >
        <Plus size={16} />
        {busy ? "Creating…" : workspaceTarget?.teamId ? "Create team plan" : "Create plan & orchestrator"}
      </button>
      {error && (
        <div className="text-xs text-center max-w-sm" style={{ color: "var(--ct-error, #f87171)" }}>
          {error}
        </div>
      )}
    </div>
  );
}
