import { authHeaders, baseUrl } from "../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../lib/types";
import type { Plan, PlanStatus } from "./types";

// Lightweight list row (GET /api/plans) — enough for PlanButton visibility/pulse
// without fetching full plan bodies.
export interface PlanIndexEntry {
  plan_id: string;
  file_path: string;
  owner: Plan["owner"];
  title: string;
  status: PlanStatus;
  updated_at: number;
  assigned_pane_ids: string[];
}

interface PlanIndexResponse {
  plans: PlanIndexEntry[];
}

interface PlansFullResponse {
  plans: Plan[];
}

export async function fetchPlan(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<Plan> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plan get failed: ${res.status} ${await res.text()}`);
  return res.json();
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

// Full plan bodies in ONE batched request (?full=1) — no N+1 per-plan fan-out.
export async function fetchPlans(api: ApiConfig, signal?: AbortSignal): Promise<Plan[]> {
  const res = await fetch(`${baseUrl(api)}/api/plans?full=1`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlansFullResponse;
  return body.plans ?? [];
}
