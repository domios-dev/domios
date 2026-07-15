import { useCallback, useState } from "react";
import { Plus } from "lucide-react";
import type { ApiConfig, ProviderDescriptor } from "../../lib/types";
import { useConfig } from "../../hooks/useConfig";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMuxState } from "../../store/multiplexer";
import { useCanvasWorkspaceTarget, type CanvasWorkspaceTarget } from "./canvasFocusStore";
import { createCanvasPlanRequest, resolveCanvasSpawnProvider, spawnOrchestrator } from "./canvasApi";
import type { CreatePlanParams } from "./plansApi";

export interface CanvasEmptyActionsProps {
  api: ApiConfig;
  projectPath?: string | null;
  workspaceId?: string | null;
}

interface CreateCanvasPlanArgs {
  api: ApiConfig;
  projectPath?: string | null;
  workspaceId?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  providerId?: string;
  selectedProviderId?: string | null;
  providers?: readonly ProviderDescriptor[];
  spawnOrchestrator: (api: ApiConfig, options: { providerId: string; workspaceId: string }) => Promise<string | null>;
  createPlan: (api: ApiConfig, params: CreatePlanParams) => Promise<unknown>;
}

interface CreatedPlanOwnership {
  plan?: {
    owner?: { pane_id?: string | null };
    team_id?: string | null;
  };
  team_id?: string | null;
}

function verifyCreatedOwnership(created: unknown, paneId: string): void {
  const result = created as CreatedPlanOwnership;
  if (result.plan?.owner?.pane_id !== paneId) {
    throw new Error("Created plan owner does not match the Canvas orchestrator");
  }
  if (!result.team_id || result.plan?.team_id !== result.team_id) {
    throw new Error("Created plan Team binding does not match the minted Team");
  }
}

export async function createCanvasPlan({
  api,
  projectPath,
  workspaceId,
  workspaceTarget,
  providerId,
  selectedProviderId,
  providers,
  spawnOrchestrator: spawn,
  createPlan,
}: CreateCanvasPlanArgs): Promise<void> {
  const isTeamTarget = workspaceTarget?.teamId != null;
  const paneId = isTeamTarget
    ? workspaceTarget?.managerPaneId ?? null
    : await (() => {
      if (!workspaceId) throw new Error("Canvas orchestrator spawn requires a workspace");
      const resolvedProvider = providerId ?? resolveCanvasSpawnProvider(selectedProviderId, providers ?? []);
      return spawn(api, { providerId: resolvedProvider, workspaceId });
    })();
  const created = await createPlan(api, {
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
  if (!isTeamTarget && paneId) verifyCreatedOwnership(created, paneId);
}

export function CanvasEmptyActions({ api, projectPath, workspaceId }: CanvasEmptyActionsProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const workspaceTarget = useCanvasWorkspaceTarget();
  const { config } = useConfig();
  const { providers } = useAgentProviders();
  const muxState = useMuxState();
  const actingWorkspaceId = workspaceId ?? muxState.active_workspace_id;

  const onCreate = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await createCanvasPlan({
        api,
        projectPath,
        workspaceId: actingWorkspaceId,
        workspaceTarget,
        selectedProviderId: config.general_agent?.agent_provider,
        providers,
        spawnOrchestrator,
        createPlan: createCanvasPlanRequest,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [actingWorkspaceId, api, busy, config.general_agent?.agent_provider, projectPath, providers, workspaceTarget]);

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
