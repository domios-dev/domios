import type { JobsTarget } from "./JobsView";
import type { ReturnToRoute } from "../../lib/returnTo";

export const OPEN_JOB_EVENT = "codeterm:open-job";

export interface OpenJobDetail extends ReturnToRoute {
  workspaceId: string;
  target: JobsTarget;
}

export function requestOpenJob(detail: OpenJobDetail): void {
  window.dispatchEvent(new CustomEvent<OpenJobDetail>(OPEN_JOB_EVENT, { detail }));
}
