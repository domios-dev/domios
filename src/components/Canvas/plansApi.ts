import { authHeaders, baseUrl } from "../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../lib/types";
import type { Plan, PlanStatus } from "./types";

export interface PlanIndexEntry {
  plan_id: string;
  file_path: string;
  owner: Plan["owner"];
  title: string;
  status: PlanStatus;
  updated_at: number;
}

interface PlansListResponse {
  plans: PlanIndexEntry[];
}

export async function fetchPlan(api: ApiConfig, planId: string, signal?: AbortSignal): Promise<Plan> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plan get failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchPlans(api: ApiConfig, signal?: AbortSignal): Promise<Plan[]> {
  const res = await fetch(`${baseUrl(api)}/api/plans`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plans list failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlansListResponse;
  const entries = body.plans ?? [];
  if (entries.length === 0) return [];
  const plans = await Promise.all(
    entries.map((entry) => fetchPlan(api, entry.plan_id, signal)),
  );
  return plans;
}
