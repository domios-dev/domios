import type { JobsTarget } from "./JobsView";

export const OPEN_JOB_EVENT = "codeterm:open-job";

export interface OpenJobDetail {
  workspaceId: string;
  target: JobsTarget;
}

export function requestOpenJob(detail: OpenJobDetail): void {
  window.dispatchEvent(new CustomEvent<OpenJobDetail>(OPEN_JOB_EVENT, { detail }));
}
