import { invoke } from "../../lib/bridge";
import type { CronJob } from "../../lib/types";
import type { AutomationGraphAdapter } from "../Automation/types";
import type { MergedJob } from "./jobsModel";
import type { WorkspaceGraph } from "./useWorkspaceGraphs";

export interface JobsActionAdapter {
  stopCron(job: CronJob): Promise<unknown>;
  deleteCron(job: CronJob): Promise<unknown>;
}

export const hostJobsActionAdapter: JobsActionAdapter = {
  stopCron: (job) => invoke("cron_cancel", { id: job.id }),
  deleteCron: (job) => invoke("cron_delete", { id: job.id }),
};

export interface JobActionContext {
  adapter: AutomationGraphAdapter;
  jobActions: JobsActionAdapter;
  graphFor: (workspaceId: string) => WorkspaceGraph | undefined;
  refreshCrons: () => Promise<unknown>;
}

export function jobWorkspace(job: MergedJob): string {
  if (job.kind === "blueprint") return job.blueprint.identity.workspace_id;
  return job.kind === "cron" ? job.cron.workspace_id : job.sentinel.scenario.root.workspace_id;
}

export async function deleteMergedJob(job: MergedJob, context: JobActionContext): Promise<unknown> {
  if (job.kind === "cron") return context.jobActions.deleteCron(job.cron);
  const graph = context.graphFor(jobWorkspace(job));
  if (!graph) throw new Error("Sentinel workspace is not attached");
  return context.adapter.delete(graph.attachment, job.kind === "blueprint" ? job.blueprint.identity : job.sentinel.scenario.root);
}

export async function stopMergedJob(job: MergedJob, context: JobActionContext): Promise<void> {
  if (job.kind === "cron") {
    await context.jobActions.stopCron(job.cron);
    await context.refreshCrons();
    return;
  }
  const identity = job.kind === "blueprint" ? job.blueprint.identity : job.sentinel.root?.identity;
  const graph = context.graphFor(jobWorkspace(job));
  if (!identity || !graph) return;
  await context.adapter.stop(graph.attachment, identity);
  await graph.refresh();
}

export function deleteJobConfirmation(job: MergedJob) {
  return {
    title: `Delete ${job.title}?`,
    description: `Delete ${job.title} and its history? This cannot be undone.`,
    confirmLabel: "Delete",
    error: `Could not delete ${job.title}.`,
  };
}
