import { useCallback, useState } from "react";
import { Plus } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { baseUrl, jsonHeaders } from "../MemoryPanel/memoryApi";
import { createPlan } from "./plansApi";

export interface CanvasEmptyActionsProps {
  api: ApiConfig;
}

export function CanvasEmptyActions({ api }: CanvasEmptyActionsProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const spawnOrchestrator = useCallback(async (): Promise<string | null> => {
    const res = await fetch(`${baseUrl(api)}/api/agents/spawn`, {
      method: "POST",
      headers: jsonHeaders(api),
      body: JSON.stringify({
        agent: "claude",
        title: "Canvas Orchestrator",
        task: "You are the orchestrator for a new plan on the workspace canvas.",
        activate: true,
      }),
    });
    if (!res.ok) throw new Error(`spawn failed: ${res.status}`);
    const body = (await res.json()) as { id?: string; pane_id?: string };
    return body.id ?? body.pane_id ?? null;
  }, [api]);

  const onCreate = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const paneId = await spawnOrchestrator();
      await createPlan(api, {
        title: "New plan",
        owner: paneId
          ? { pane_id: paneId, session_id: null, title: "Canvas Orchestrator" }
          : undefined,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [api, busy, spawnOrchestrator]);

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
        {busy ? "Creating…" : "Create plan & orchestrator"}
      </button>
      {error && (
        <div className="text-xs text-center max-w-sm" style={{ color: "var(--ct-error, #f87171)" }}>
          {error}
        </div>
      )}
    </div>
  );
}
