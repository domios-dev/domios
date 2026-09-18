import { useCallback, useState } from "react";
import { Plus } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { useCanvasWorkspaceTarget, type CanvasWorkspaceTarget } from "./canvasFocusStore";
import { createCanvasPlanRequest } from "./canvasApi";
import type { CreatePlanParams } from "./plansApi";

export interface CanvasEmptyActionsProps {
  api: ApiConfig;
  projectPath?: string | null;
}

interface CreateCanvasPlanArgs {
  api: ApiConfig;
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  createPlan: (api: ApiConfig, params: CreatePlanParams) => Promise<unknown>;
}

interface CreatedPlanOwnership {
  plan?: {
    owner?: { tab_id?: string | null };
    team_id?: string | null;
  };
  team_id?: string | null;
}

function verifyCreatedOwnership(created: unknown, tabId: string, expectedTeamId?: string): void {
  const result = created as CreatedPlanOwnership;
  if (result.plan?.owner?.tab_id !== tabId) {
    throw new Error("Created plan owner does not match the Canvas orchestrator");
  }
  if (!result.team_id || result.plan?.team_id !== result.team_id) {
    throw new Error("Created plan Team binding does not match the minted Team");
  }
  if (expectedTeamId && result.team_id !== expectedTeamId) {
    throw new Error("Created plan Team binding does not match the selected Team");
  }
}

export async function createCanvasPlan({
  api,
  projectPath,
  workspaceTarget,
  createPlan,
}: CreateCanvasPlanArgs): Promise<void> {
  const teamId = workspaceTarget?.teamId;
  const tabId = workspaceTarget?.managerTabId;
  if (!teamId || !tabId) throw new Error("Plan creation requires a Team manager context");
  const created = await createPlan(api, {
    title: "New plan",
    project_path: projectPath ?? undefined,
    owner: tabId
      ? {
        tab_id: tabId,
        session_id: null,
        title: "Team Manager",
      }
      : undefined,
  });
  verifyCreatedOwnership(created, tabId, teamId);
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
        createPlan: createCanvasPlanRequest,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [api.host, api.port, api.token, busy, projectPath, workspaceTarget]);

  if (!workspaceTarget?.teamId || !workspaceTarget.managerTabId) return null;

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
        {busy ? "Creating…" : "Create Team plan"}
      </button>
      {error && (
        <div className="text-xs text-center max-w-sm" style={{ color: "var(--ct-red)" }}>
          {error}
        </div>
      )}
    </div>
  );
}
