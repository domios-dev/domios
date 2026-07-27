import { authHeaders, baseUrl, jsonHeaders } from "../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../lib/types";
import type { DriftItem, OrchestratorRef, Plan, PlanStatus, Section } from "./types";

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
