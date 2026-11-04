import type { PlanView as Plan } from "../Canvas/planProjection";
import type { ApiConfig } from "../../lib/types";
import { assertWebRequestContext, webRequestContext } from "../../lib/webApi";

import { fetchRequestedPlanTaskMetadata } from "../Canvas/plansApi";

export async function readTeamClosePlanMetadata(api: ApiConfig, headers: readonly Plan[]): Promise<Plan[]> {
  const context = webRequestContext();
  const result: Plan[] = [];
  for (const plan of headers) {
    assertWebRequestContext(context);
    result.push(await fetchRequestedPlanTaskMetadata(api,plan.id));
    assertWebRequestContext(context);
  }
  return result;
}
