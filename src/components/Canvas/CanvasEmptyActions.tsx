import { createCanvasPlan } from "./createCanvasPlan";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useState } from "react";
import { Plus } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { useCanvasWorkspaceTarget } from "./canvasFocusStore";
import { createCanvasPlanRequest } from "./canvasApi";

export interface CanvasEmptyActionsProps {
  api: ApiConfig;
  projectPath?: string | null;
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
        <InlineErrorNotice error={error} operation="starting this plan" />
      )}
    </div>
  );
}
