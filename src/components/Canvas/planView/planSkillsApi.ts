import { daemonFetch } from "../../../lib/webApi";
import { authHeaders } from "../../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../../lib/types";

export type PlanSkillSummary = import("../../../../gen/PlanSkillSummary").PlanSkillSummary;

export type PlanSkillLedgerEntry = import("../../../../gen/PlanSkillLedgerEntry").PlanSkillLedgerEntry;

export type PlanSkillDetail = import("../../../../gen/PlanSkillDetail").PlanSkillDetail;

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
