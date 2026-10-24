import { webFetch } from "../../lib/webApi";
import type { MergedJob } from "./jobsModel";

export interface SavedSolution { id: string; workspaceId: string; when_to_use: string; intents?: readonly string[]; tags?: readonly string[]; how: string | null; uses: number; successes: number; last_used: number }
/** The stored memory skill behind a saved solution: its Blueprint YAML body and catalogue tags. */
export interface SolutionDocument { title: string; body: string; tags: readonly string[] }
export interface SolutionScope { workspace_id: string; team_id: string | null; id: string }
export interface BlueprintSolutionsApi {
  list(workspaceId: string): Promise<Omit<SavedSolution, "workspaceId">[]>;
  save(scope: SolutionScope, when: string, tags: readonly string[]): Promise<{ id: string; status: "saved" }>;
  archive(workspaceId: string, id: string): Promise<unknown>;
  document(id: string): Promise<SolutionDocument>;
  search(workspaceId: string, query: string): Promise<readonly string[]>;
}
async function request<T>(action: string, body: object, confirmation?: string): Promise<T> {
  const response = await webFetch(`/api/automation/blueprints/solutions/${action}`, {
    method: "POST", headers: { "Content-Type": "application/json", ...(confirmation ? { "X-CodeTerm-Blueprint-Confirmation": confirmation } : {}) }, body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json() as Promise<T>;
}
export const blueprintSolutionsApi: BlueprintSolutionsApi = {
  list: (workspaceId) => request("list", { workspace_id: workspaceId }),
  save: (scope, when, tags) => request("save", { ...scope, when_to_use: when, tags }, scope.id),
  archive: (workspaceId, id) => request("archive", { workspace_id: workspaceId, team_id: null, id }, id),
  document: async (id) => {
    const response = await webFetch(`/api/mem/items/${encodeURIComponent(id)}`);
    if (!response.ok) throw new Error(await response.text());
    const item = await response.json() as { title: string; body: string; tags?: string[] };
    return { title: item.title, body: item.body, tags: item.tags ?? [] };
  },
  search: async (workspaceId, query) => {
    const params = new URLSearchParams({ q: query, kind: "skill", area: `blueprints:${workspaceId}`, limit: "50" });
    const response = await webFetch(`/api/mem/search?${params}`);
    if (!response.ok) throw new Error(await response.text());
    const result = await response.json() as { hits?: { id?: string }[] };
    return (result.hits ?? []).flatMap((hit) => hit.id ? [hit.id] : []);
  },
};
export function successfulSolutionScope(job: MergedJob): SolutionScope | null {
  if (job.kind === "cron") return null;
  if (job.kind === "sentinel") return job.sentinel.root?.solution_success ? {
    workspace_id: job.sentinel.scenario.root.workspace_id, team_id: job.sentinel.scenario.root.team_id, id: job.sentinel.scenario.root.id,
  } : null;
  const pipeline = job.blueprint.pipeline;
  const delivered = (view: typeof pipeline) => view.nodes.some((node) => node.kind === "result" && node.state === "delivered");
  return delivered(pipeline) || pipeline.runs?.some((run) => delivered(run.pipeline))
    ? { workspace_id: job.blueprint.identity.workspace_id, team_id: job.blueprint.identity.team_id, id: job.blueprint.identity.id } : null;
}
