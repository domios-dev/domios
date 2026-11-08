import { webRequestContext } from "../../lib/webApi";
import type { JobsFilter, JobsIndicator } from "../Jobs/jobsModel";

const known = new Map<string, JobsIndicator>();
let knownFor = -1;

/** Each scope's last counted indicator on this machine, outliving the mounts that come and go with each switch. */
function indicators() {
  const context = webRequestContext();
  if (context !== knownFor) {
    known.clear();
    knownFor = context;
  }
  return known;
}

const key = (scope: JobsFilter) => JSON.stringify([scope.workspaceId, scope.teamId]);

export function lastKnownJobs(scope: JobsFilter): JobsIndicator | null {
  return indicators().get(key(scope)) ?? null;
}

export function rememberJobs(scope: JobsFilter, indicator: JobsIndicator) {
  indicators().set(key(scope), indicator);
}
