import { authHeaders, baseUrl, jsonHeaders } from "../MemoryPanel/memoryApi";
import { invoke, IS_WEB } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import type { DriftItem, OrchestratorRef, Plan, PlanMode, PlanStatus, Section } from "./types";
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

interface PlansFullResponse {
  plans: Plan[];
}

async function invokePlanRead<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  signal?: AbortSignal,
): Promise<T> {
  if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
  const value = await invoke<T>(command, args);
  if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
  return value;
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

export async function fetchPlan(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<Plan> {
  if (!IS_WEB) return invokePlanRead("get_plan_full", { planId }, signal);
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plan get failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchSpecSections(api: ApiConfig, specId: string, signal?: AbortSignal): Promise<Section[]> {
  const res = await fetch(`${baseUrl(api)}/api/specs/${encodeURIComponent(specId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`spec get failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as { spec?: { sections?: Section[] } };
  return body.spec?.sections ?? [];
}

export async function fetchReconcile(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<DriftItem[]> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/reconcile`, {
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
  if (!IS_WEB) return invokePlanRead("list_plan_index", undefined, signal);
  const res = await fetch(`${baseUrl(api)}/api/plans`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlanIndexResponse;
  return body.plans ?? [];
}

export async function resolvePlanRefs(
  api: ApiConfig,
  refs: readonly PlanRefRequest[],
  signal?: AbortSignal,
): Promise<ResolvedPlanRef[]> {
  const res = await fetch(`${baseUrl(api)}/api/plans/resolve-refs`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ refs }),
    signal,
  });
  if (!res.ok) throw new Error(`plan ref resolve failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as ResolveRefsResponse;
  return body.refs ?? [];
}

// Full plan bodies in ONE batched request (?full=1) — no N+1 per-plan fan-out.
export async function fetchPlans(
  api: ApiConfig,
  signal?: AbortSignal,
  opts?: { project?: string | null },
): Promise<Plan[]> {
  if (!IS_WEB) {
    return invokePlanRead("list_plans_full", { project: opts?.project ?? null }, signal);
  }
  const params = new URLSearchParams({ full: "1" });
  if (opts?.project) params.set("project", opts.project);
  const res = await fetch(`${baseUrl(api)}/api/plans?${params}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlansFullResponse;
  return body.plans ?? [];
}

export async function fetchUnownedPlanIndex(
  api: ApiConfig,
  signal?: AbortSignal,
): Promise<PlanIndexEntry[]> {
  const res = await fetch(`${baseUrl(api)}/api/plans?unowned=1`, {
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

export async function createPlan(api: ApiConfig, params: CreatePlanParams): Promise<Plan> {
  const res = await fetch(`${baseUrl(api)}/api/plans`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify(params),
  });
  if (!res.ok) throw new Error(`plan create failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as { plan_id?: string; plan?: Plan };
  if (body.plan) return body.plan;
  if (body.plan_id) return fetchPlan(api, body.plan_id);
  throw new Error("plan create returned no plan");
}

export async function patchPlanLayout(
  api: ApiConfig,
  planId: string,
  elementId: string,
  x: number,
  y: number,
): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/layout`, {
    method: "PATCH",
    headers: jsonHeaders(api),
    body: JSON.stringify({ element: elementId, x, y }),
  });
  if (!res.ok) throw new Error(`plan layout failed: ${res.status} ${await res.text()}`);
}

export async function archivePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/archive`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan archive failed: ${res.status} ${await res.text()}`);
}

export async function setPlanMode(api: ApiConfig, planId: string, mode: PlanMode): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/mode`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ mode }),
  });
  if (!res.ok) throw new Error(`plan mode failed: ${res.status} ${await res.text()}`);
}

export async function deletePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}`, {
    method: "DELETE",
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`plan delete failed: ${res.status} ${await res.text()}`);
}

export async function deleteTask(api: ApiConfig, planId: string, taskId: string): Promise<void> {
  const res = await fetch(
    `${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}`,
    { method: "DELETE", headers: authHeaders(api) },
  );
  if (!res.ok) throw new Error(`task delete failed: ${res.status} ${await res.text()}`);
}

export async function pauseTask(api: ApiConfig, planId: string, taskId: string, reason: string): Promise<void> {
  const res = await fetch(
    `${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}/pause`,
    { method: "POST", headers: jsonHeaders(api), body: JSON.stringify({ reason }) },
  );
  if (!res.ok) throw new Error(`task pause failed: ${res.status} ${await res.text()}`);
}

export async function resumeTask(api: ApiConfig, planId: string, taskId: string): Promise<void> {
  const res = await fetch(
    `${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/tasks/${encodeURIComponent(taskId)}/resume`,
    { method: "POST", headers: jsonHeaders(api), body: "{}" },
  );
  if (!res.ok) throw new Error(`task resume failed: ${res.status} ${await res.text()}`);
}

export async function unarchivePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/unarchive`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan unarchive failed: ${res.status} ${await res.text()}`);
}

export async function parkPlan(api: ApiConfig, planId: string, reason: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/pause`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ reason }),
  });
  if (!res.ok) throw new Error(`plan park failed: ${res.status} ${await res.text()}`);
}

export async function fetchPlanLifecyclePreview(
  api: ApiConfig,
  planId: string,
  kind: PlanLifecycleKind,
): Promise<PlanLifecyclePreview> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/${kind}/preview`, {
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`plan ${kind} preview failed: ${res.status} ${await res.text()}`);
  return (await res.json()) as PlanLifecyclePreview;
}

export async function resumePlan(api: ApiConfig, planId: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/resume`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: "{}",
  });
  if (!res.ok) throw new Error(`plan resume failed: ${res.status} ${await res.text()}`);
}

export async function renamePlan(api: ApiConfig, planId: string, title: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}`, {
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
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/claim`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ owner }),
  });
  if (!res.ok) throw new Error(`plan claim failed: ${res.status} ${await res.text()}`);
}
