import { daemonFetch } from "../../../lib/webApi";
import { authHeaders } from "../../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../../lib/types";

export interface PlanSkillSummary {
  slug: string;
  title: string;
  purpose: string;
  check: string | null;
  verdict: string;
}

export interface PlanSkillLedgerEntry {
  seq: number;
  slug: string;
  verdict: string;
  rationale: string;
}

export interface PlanSkillDetail {
  slug: string;
  title: string;
  body: string;
  ledger: PlanSkillLedgerEntry[];
}

export async function fetchPlanSkills(
  api: ApiConfig,
  planId: string,
  signal?: AbortSignal,
): Promise<PlanSkillSummary[]> {
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/skills`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plan skills list failed: ${res.status}`);
  const body = (await res.json()) as { skills?: PlanSkillSummary[] };
  return body.skills ?? [];
}

export async function fetchPlanSkill(
  api: ApiConfig,
  planId: string,
  slug: string,
  signal?: AbortSignal,
): Promise<PlanSkillDetail> {
  const res = await daemonFetch(
    api,
    `/api/plans/${encodeURIComponent(planId)}/skills/${encodeURIComponent(slug)}`,
    { headers: authHeaders(api), signal },
  );
  if (!res.ok) throw new Error(`plan skill show failed: ${res.status}`);
  const body = (await res.json()) as { skill?: PlanSkillDetail };
  if (!body.skill) throw new Error("plan skill missing from response");
  return body.skill;
}
