import type { JobBlockHost, JobBlockState } from "@codeterm/chat-ui";
import type { AutomationAttachment, AutomationIdentity } from "../../components/Automation/types";
import { ALL_JOBS_SCOPE } from "../../components/Automation/types";
import type { OpenJobDetail } from "../../components/Jobs/openJobEvent";
import type { JobsBlockStore } from "./jobsBlockStore";
import { jobBlockState, resolveJob } from "./jobBlockState";

/** The Jobs view's own commands; the card adds no new path to the daemon. */
export interface JobCommands {
  triggerCron(id: string): Promise<unknown>;
  toggleCron(id: string, enabled: boolean): Promise<unknown>;
  cancelCron(id: string): Promise<unknown>;
  stopBlueprint(attachment: AutomationAttachment, identity: AutomationIdentity): Promise<void>;
  openJob(detail: OpenJobDetail): void;
}

export function createJobBlockHost(store: JobsBlockStore, commands: JobCommands): JobBlockHost {
  let cachedFor = store.get();
  const states = new Map<string, JobBlockState>();

  const resolve = (ref: string) => {
    const job = resolveJob(store.get(), ref);
    if (!job) throw new Error("this job is no longer listed");
    return job;
  };
  const after = async (action: Promise<unknown>) => {
    try {
      await action;
    } finally {
      await store.refresh();
    }
  };

  return {
    binding: {
      subscribe: store.subscribe,
      read(ref) {
        const snapshot = store.get();
        if (snapshot !== cachedFor) {
          cachedFor = snapshot;
          states.clear();
        }
        let state = states.get(ref);
        if (!state) {
          state = jobBlockState(snapshot, ref);
          states.set(ref, state);
        }
        return state;
      },
      refresh: () => void store.refresh(),
    },
    open(ref) {
      const job = resolveJob(store.get(), ref);
      if (!job) return;
      if (job.kind === "cron") commands.openJob({ workspaceId: job.cron.workspace_id, target: { teamId: job.cron.team_id ?? null, cronId: job.cron.id } });
      else commands.openJob({ workspaceId: job.workspaceId, target: { teamId: job.job.identity.team_id, blueprintId: job.snapshot.id } });
    },
    async runNow(ref) {
      const job = resolve(ref);
      if (job.kind !== "cron") throw new Error("Blueprints start from their own trigger");
      await after(commands.triggerCron(job.cron.id));
    },
    async setPaused(ref, paused) {
      const job = resolve(ref);
      if (job.kind !== "cron") throw new Error("Blueprints cannot be paused");
      await after(commands.toggleCron(job.cron.id, !paused));
    },
    async stop(ref) {
      const job = resolve(ref);
      if (job.kind === "cron") {
        await after(commands.cancelCron(job.cron.id));
        return;
      }
      const attachment = { connection_id: "chat-block", workspace_id: job.workspaceId, team_id: ALL_JOBS_SCOPE };
      await after(commands.stopBlueprint(attachment, job.job.identity));
    },
  };
}
