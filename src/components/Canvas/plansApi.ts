import { daemonFetch, webRequestContext, assertWebRequestContext } from "../../lib/webApi";
import type { ControlLoopUpdate, PlanControlLoop } from "./planControlLoop";
import { authHeaders, jsonHeaders } from "../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../lib/types";
import { planRevision, type PlanView, type PlanProjection, type PlanProjectionView, type PlanListProjection, type PlanListCursor } from "./planProjection";
import type {
  DriftItem,
  OrchestratorRef,
  Plan,
  PlanMode,
  PlanStatus,
  PlanTaskBoardColumnKey,
  PlanTaskBoardCursor,
  PlanTaskBoardPage,
  PlanTaskDetail,
  PlanTaskHistoryPage,
  TaskHistoryKind,
  Section,
} from "./types";
import type { PlanLifecycleKind, PlanLifecyclePreview } from "./planLifecycle";

// without fetching full plan bodies.
export interface PlanIndexEntry {
  plan_id: string;
  file_path: string;
  canonical_project_path?: string;
  owner: Plan["owner"];
  title: string;
  status: PlanStatus;
  updated_at: number;
  team_id?: string | null;
  assigned_tab_ids: string[];
  task_count?: number;
  open_task_count?: number;
  section_count?: number;
  element_count?: number;
  loop_count?: number;
  has_goal?: boolean;
}

interface PlanIndexResponse {
  plans: PlanIndexEntry[];
}

interface PlansProjectionResponse {
  plans: PlanProjection[];
}

export type PlanRefTargetKind = "plan" | "task" | "section";

export interface PlanRefRequest {
  plan_id: string;
  target_id?: string;
  target_kind: PlanRefTargetKind;
}

export interface ResolvedPlanRef {
  plan_id: string;
  target_id?: string;
  target_kind: PlanRefTargetKind;
  state: "resolved" | "archived" | "unowned" | "stale" | "missing" | "unreachable" | "permission";
  title: string;
  plan_title: string;
  task_title?: string;
  /** The task's display key (`PREFIX-N`, or the legacy id fallback) when the task resolved. */
  task_key?: string;
  section_title?: string;
  file_path?: string;
  canonical_project_path?: string;
  status?: PlanStatus;
  owner_tab_id?: string;
  owner_session_id?: string;
}

interface ReconcileReport {
  drift: DriftItem[];
  fixed: [];
  accepted_signal: string;
}

interface ResolveRefsResponse {
  refs: ResolvedPlanRef[];
}

export class PlanReadError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
  }
}

export async function fetchPlanProjection(api: ApiConfig, planId: string, view: PlanProjectionView, signal?: AbortSignal, project?: string | null): Promise<PlanProjection> {
  const query = new URLSearchParams({ view });
  if (project) query.set("project", project);
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/projection?${query}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new PlanReadError(res.status, `plan projection failed: ${res.status} ${await res.text()}`);
  const value = await res.json() as PlanProjection;
  planRevision(value);
  if (value.id !== planId) throw new Error("Plan projection did not match the requested plan.");
  return value;
}

export async function fetchPlanListWindow(api: ApiConfig, planId: string, status: string, search: string, signal?: AbortSignal, cursor?: PlanListCursor, taskRef?: string, taskIds?: readonly string[], metadata = false): Promise<PlanListProjection> {
  const query = new URLSearchParams({ view: metadata ? "task_metadata" : "list", page_size: "25", status, search });
  if (taskIds) query.set("task_ids",JSON.stringify(taskIds));
  if (taskRef) query.set("task_ref", taskRef);
  if (cursor) { query.set("after_position", String(cursor.position)); query.set("after_task_id", cursor.task_id); }
  const response = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/projection?${query}`, { headers: authHeaders(api), signal });
  if (!response.ok) throw new PlanReadError(response.status, `plan list failed: ${response.status} ${await response.text()}`);
  const value = await response.json() as PlanListProjection;
  planRevision(value);
  if (value.id !== planId || !value.task_window) throw new Error("Plan list did not match the requested window.");
  return value;
}

export async function fetchRequestedPlanTaskMetadata(api: ApiConfig, planId: string, status = "all", taskRef?: string, signal?: AbortSignal): Promise<PlanListProjection> {
  const context = webRequestContext();
  let cursor: PlanListCursor | undefined;
  let result: PlanListProjection | undefined;
  const tasks = new Map<string, PlanProjection["tasks"][number]>();
  do {
    assertWebRequestContext(context);
    const page = await fetchPlanListWindow(api, planId, status, "", signal, cursor, taskRef, undefined, true);
    assertWebRequestContext(context);
    if (result && page.revision !== result.revision) throw new Error("Tasks changed while reading the requested list. Read it again to use the current tasks.");
    for (const task of page.tasks) tasks.set(task.id, task);
    const next = page.task_window.next_cursor;
    if (cursor && next && (next.position < cursor.position || next.position === cursor.position && next.task_id <= cursor.task_id)) throw new Error("Task list cursor did not advance.");
    result = page;
    cursor = next ?? undefined;
  } while (cursor);
  return { ...result!, tasks: [...tasks.values()] };
}

export interface PlanTaskReference {plan_id:string;task_id:string;name:string;key:string;team_id:string|null;revision:string}
export async function fetchPlanTaskReference(api:ApiConfig,reference:string,project:string|null,team:string|null,signal?:AbortSignal):Promise<PlanTaskReference|null>{
  const query=new URLSearchParams({reference});
  if(project)query.set("project",project);
  if(team)query.set("team",team);
  const response=await daemonFetch(api,`/api/plans/task-reference?${query}`,{headers:authHeaders(api),signal});
  if(response.status===404)return null;
  if(!response.ok)throw new PlanReadError(response.status,`task reference failed: ${response.status} ${await response.text()}`);
  const value=await response.json() as PlanTaskReference;
  planRevision(value);return value;
}

export async function fetchPlanTaskDetail(
  api: ApiConfig,
  planId: string,
  taskId: string,
  signal?: AbortSignal,
): Promise<PlanTaskDetail> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new PlanReadError(res.status, `plan task detail failed: ${res.status} ${await res.text()}`);
  const value = await res.json() as PlanTaskDetail;
  planRevision(value);
  return value;
}

export async function fetchPlanTaskHistory(api: ApiConfig, planId: string, taskId: string, kind: TaskHistoryKind, before: number, signal?: AbortSignal): Promise<PlanTaskHistoryPage> {
  const params = new URLSearchParams({ kind, before: String(before), limit: "50" });
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}/history?${params}`, { headers: authHeaders(api), signal });
  if (!res.ok) throw new PlanReadError(res.status, `task history failed: ${res.status} ${await res.text()}`);
  const page = await res.json() as PlanTaskHistoryPage;
  planRevision(page);
  if (page.plan_id !== planId || page.task_id !== taskId) throw new Error("Task history did not match the selected task.");
  return page;
}

export async function fetchPlanTaskBoardPage(
  api: ApiConfig,
  planId: string,
  options: {
    signal?: AbortSignal;
    status?: PlanTaskBoardColumnKey | "cancelled";
    cursor?: PlanTaskBoardCursor | null;
    pageSize?: number;
  } = {},
): Promise<PlanTaskBoardPage> {
  const params = new URLSearchParams({ page_size: String(options.pageSize ?? 25) });
  if (options.status) params.set("status", options.status);
  if (options.cursor) {
    params.set("after_created_at", String(options.cursor.created_at));
    params.set("after_position", String(options.cursor.position));
    params.set("after_task_id", options.cursor.task_id);
  }
  const res = await daemonFetch(
    api,
    `/api/plans/${encodeURIComponent(planId)}/tasks?${params}`,
    { headers: authHeaders(api), signal: options.signal },
  );
  if (!res.ok) throw new PlanReadError(res.status, `plan task page failed: ${res.status} ${await res.text()}`);
  const value = await res.json() as PlanTaskBoardPage;
  planRevision(value);
  return value;
}

export async function fetchSpecSections(api: ApiConfig, specId: string, signal?: AbortSignal): Promise<Section[]> {
  const res = await daemonFetch(api, `/api/specs/${encodeURIComponent(specId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`spec get failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as { spec?: { sections?: Section[] } };
  return body.spec?.sections ?? [];
}

export async function fetchReconcile(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<DriftItem[]> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/reconcile`, {
    headers: authHeaders(api),
    signal,
  });
  if (res.status === 404) return [];
  if (!res.ok) throw new Error(`plan reconcile failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as ReconcileReport;
  return body.drift ?? [];
}

// Lightweight index only — one request, no per-plan bodies.
export async function fetchPlanIndex(api: ApiConfig, signal?: AbortSignal): Promise<PlanIndexEntry[]> {
  const res = await daemonFetch(api, `/api/plans`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new PlanReadError(res.status, `plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlanIndexResponse;
  return body.plans ?? [];
}

export async function resolvePlanRefs(
  api: ApiConfig,
  refs: readonly PlanRefRequest[],
  signal?: AbortSignal,
): Promise<ResolvedPlanRef[]> {
  const res = await daemonFetch(api, `/api/plans/resolve-refs`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ refs }),
    signal,
  });
  if (!res.ok) throw new Error(`plan ref resolve failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as ResolveRefsResponse;
  return body.refs ?? [];
}

export async function fetchPlans(
  api: ApiConfig,
  signal?: AbortSignal,
  opts?: { project?: string | null; chat?: boolean; query?: string; team?: string | null; tabId?: string },
): Promise<PlanProjection[]> {
  const params = new URLSearchParams(opts?.chat ? { chat: "1" } : { view: "summary" });
  if (opts?.project) params.set("project", opts.project);
  if (opts?.tabId) params.set("tab_id", opts.tabId);
  if (opts?.query !== undefined) params.set("query", opts.query);
  if (opts?.team !== undefined) params.set("team", opts.team ?? "");
  const res = await daemonFetch(api, `/api/plans?${params}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new PlanReadError(res.status, `plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlansProjectionResponse;
  const plans = body.plans ?? [];
  for (const plan of plans) planRevision(plan);
  return plans;
}

export async function fetchUnownedPlanIndex(
  api: ApiConfig,
  signal?: AbortSignal,
): Promise<PlanIndexEntry[]> {
  const res = await daemonFetch(api, `/api/plans?unowned=1`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`unowned plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlanIndexResponse;
  return body.plans ?? [];
}

export interface CreatePlanParams {
  title: string;
  project_path?: string;
  owner?: OrchestratorRef;
  kinds?: string;
  spec?: string;
}

export async function createPlan(api: ApiConfig, params: CreatePlanParams): Promise<PlanView> {
  const res = await daemonFetch(api, `/api/plans`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify(params),
  });
  if (!res.ok) throw new Error(`plan create failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as { plan_id?: string; plan?: Plan };
  if (body.plan) return body.plan;
  if (body.plan_id) return fetchPlanProjection(api, body.plan_id, "summary");
  throw new Error("plan create returned no plan");
}

export async function patchPlanLayout(
  api: ApiConfig,
  planId: string,
  elementId: string,
  x: number,
  y: number,
): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/layout`, {
    method: "PATCH",
    headers: jsonHeaders(api),
    body: JSON.stringify({ element: elementId, x, y }),
  });
  if (!res.ok) throw new Error(`plan layout failed: ${res.status} ${await res.text()}`);
}

export async function archivePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/archive`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan archive failed: ${res.status} ${await res.text()}`);
}

export async function setPlanMode(api: ApiConfig, planId: string, mode: PlanMode): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/mode`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ mode }),
  });
  if (!res.ok) throw new Error(`plan mode failed: ${res.status} ${await res.text()}`);
}

export async function deletePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}`, {
    method: "DELETE",
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`plan delete failed: ${res.status} ${await res.text()}`);
}

export async function deleteTask(api: ApiConfig, planId: string, taskId: string): Promise<void> {
  const res = await daemonFetch(
    api,
    `/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}`,
    { method: "DELETE", headers: confirmedHeaders(api, planId) },
  );
  if (!res.ok) throw new Error(`task delete failed: ${res.status} ${await res.text()}`);
}

/** A click in the UI is the owner's explicit confirmation for a plan lifecycle change. */
function confirmedHeaders(api: ApiConfig, planId: string): HeadersInit {
  return { ...jsonHeaders(api), "X-CodeTerm-Plan-Confirmation": planId };
}

export async function fetchPlanControlLoop(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<PlanControlLoop> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/control-loop`, {
    headers: authHeaders(api), signal,
  });
  if (!res.ok) throw new Error(`plan control loop failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function setPlanControlLoop(api: ApiConfig, planId: string, update: ControlLoopUpdate): Promise<PlanControlLoop> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/control-loop`, {
    method: "POST", headers: confirmedHeaders(api, planId), body: JSON.stringify(update),
  });
  if (!res.ok) throw new Error(`plan control loop update failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function runPlanControlLoop(api: ApiConfig, planId: string): Promise<PlanControlLoop> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/control-loop/run`, {
    method: "POST", headers: confirmedHeaders(api, planId), body: "{}",
  });
  if (!res.ok) {
    const body: unknown = await res.json().catch(() => null);
    const record = body && typeof body === "object" ? body as Record<string, unknown> : null;
    const error = record?.error ?? record?.message ?? null;
    const message = error && typeof error === "object" && "message" in error ? error.message : error;
    throw new Error(typeof message === "string" && message.trim() ? message : `The Curator round could not start (HTTP ${res.status}).`);
  }
  return res.json();
}

export async function unarchivePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/unarchive`, {
    method: "POST",
    headers: confirmedHeaders(api, planId),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan unarchive failed: ${res.status} ${await res.text()}`);
}

export async function parkPlan(api: ApiConfig, planId: string, reason: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/pause`, {
    method: "POST",
    headers: confirmedHeaders(api, planId),
    body: JSON.stringify({ reason }),
  });
  if (!res.ok) throw new Error(`plan park failed: ${res.status} ${await res.text()}`);
}

export async function fetchPlanLifecyclePreview(
  api: ApiConfig,
  planId: string,
  kind: PlanLifecycleKind,
): Promise<PlanLifecyclePreview> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/${kind}/preview`, {
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`plan ${kind} preview failed: ${res.status} ${await res.text()}`);
  return (await res.json()) as PlanLifecyclePreview;
}

export async function resumePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/resume`, {
    method: "POST",
    headers: confirmedHeaders(api, planId),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan resume failed: ${res.status} ${await res.text()}`);
}

export async function renamePlan(api: ApiConfig, planId: string, title: string): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}`, {
    method: "PATCH",
    headers: jsonHeaders(api),
    body: JSON.stringify({ title }),
  });
  if (!res.ok) throw new Error(`plan rename failed: ${res.status} ${await res.text()}`);
}

export async function claimPlan(
  api: ApiConfig,
  planId: string,
  owner: OrchestratorRef,
): Promise<void> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/claim`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ owner }),
  });
  if (!res.ok) throw new Error(`plan claim failed: ${res.status} ${await res.text()}`);
}
