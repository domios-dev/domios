import { baseUrl, jsonHeaders } from "../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../lib/types";


export interface SpawnOrchestratorOptions {
  title?: string;
  task?: string;
}

export async function spawnOrchestrator(api: ApiConfig, opts: SpawnOrchestratorOptions = {}): Promise<string | null> {
  const res = await fetch(`${baseUrl(api)}/api/agents/spawn`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({
      agent: "claude",
      title: opts.title ?? "Canvas Orchestrator",
      task: opts.task ?? "You are the orchestrator for a new plan on the workspace canvas.",
      activate: true,
    }),
  });
  if (!res.ok) throw new Error(`spawn failed: ${res.status}`);
  const body = (await res.json()) as { id?: string; pane_id?: string };
  return body.id ?? body.pane_id ?? null;
}
