import { daemonFetch } from "../../lib/webApi";
import { jsonHeaders } from "../MemoryPanel/memoryApi";
import type { ApiConfig, ProviderDescriptor } from "../../lib/types";
import type { CreatePlanParams } from "./plansApi";

export interface SpawnOrchestratorOptions {
  providerId: string;
  workspaceId: string;
  title?: string;
}

export function resolveCanvasSpawnProvider(
  selectedProviderId: string | null | undefined,
  providers: readonly ProviderDescriptor[],
): string {
  const selected = selectedProviderId?.trim();
  const provider = providers.find((candidate) => candidate.id === selected)
    ?? providers[0];
  if (!provider || (provider.transport !== "pty" && provider.transport !== "chat_backend")) {
    throw new Error("No spawn-capable provider is available for the Canvas orchestrator");
  }
  return provider.id;
}

export async function spawnOrchestrator(api: ApiConfig, opts: SpawnOrchestratorOptions): Promise<string | null> {
  const res = await daemonFetch(api, `/api/agents/spawn`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({
      agent: opts.providerId,
      title: opts.title ?? "Canvas Orchestrator",
      role: "Manager",
      role_profile: "orchestrator",
      workspace_id: opts.workspaceId,
      activate: true,
    }),
  });
  if (!res.ok) throw new Error(`spawn failed: ${res.status}`);
  const body = (await res.json()) as { id?: string; tab_id?: string };
  return body.id ?? body.tab_id ?? null;
}

export async function createCanvasPlanRequest(api: ApiConfig, params: CreatePlanParams): Promise<unknown> {
  const res = await daemonFetch(api, `/api/plans`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify(params),
  });
  if (!res.ok) throw new Error(`plan create failed: ${res.status} ${await res.text()}`);
  return res.json();
}
