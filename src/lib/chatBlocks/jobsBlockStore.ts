import type { AutomationGraph } from "../../components/Automation/types";
import type { CronJob } from "../types";

export interface WorkspaceAutomation {
  workspaceId: string;
  graph: AutomationGraph;
}

export interface JobsSnapshot {
  crons: readonly CronJob[] | null;
  cronsError: string | null;
  graphs: readonly WorkspaceAutomation[] | null;
  graphsError: string | null;
}

/** The same reads the Jobs view makes: every Cron on this machine and each workspace's all-Teams graph. */
export interface JobsReader {
  listCrons(): Promise<CronJob[]>;
  readGraph(workspaceId: string): Promise<AutomationGraph>;
  workspaceIds(): readonly string[];
  /** Subscribes to the daemon events after which the job list may differ. */
  watch(changed: () => void): () => void;
}

export interface JobsBlockStore {
  get(): JobsSnapshot;
  subscribe(listener: () => void): () => void;
  refresh(): Promise<void>;
}

const EMPTY: JobsSnapshot = { crons: null, cronsError: null, graphs: null, graphsError: null };

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Shared by job cards and the checklist: reads only while something listens, and coalesces overlapping refreshes. */
export function createJobsBlockStore(reader: JobsReader): JobsBlockStore {
  let snapshot = EMPTY;
  let reading: Promise<void> | null = null;
  let dirty = false;
  let unwatch: (() => void) | null = null;
  const listeners = new Set<() => void>();

  const publish = (next: JobsSnapshot) => {
    snapshot = next;
    for (const listener of [...listeners]) listener();
  };

  const readOnce = async () => {
    const [crons, graphs] = await Promise.allSettled([
      reader.listCrons(),
      Promise.all(reader.workspaceIds().map(async (workspaceId) => ({ workspaceId, graph: await reader.readGraph(workspaceId) }))),
    ]);
    publish({
      crons: crons.status === "fulfilled" && Array.isArray(crons.value) ? crons.value : snapshot.crons,
      cronsError: crons.status === "rejected" ? message(crons.reason) : null,
      graphs: graphs.status === "fulfilled" ? graphs.value : snapshot.graphs,
      graphsError: graphs.status === "rejected" ? message(graphs.reason) : null,
    });
  };

  const refresh = (): Promise<void> => {
    if (reading) {
      dirty = true;
      return reading;
    }
    reading = (async () => {
      do {
        dirty = false;
        await readOnce();
      } while (dirty);
    })().finally(() => {
      reading = null;
    });
    return reading;
  };

  return {
    get: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      if (listeners.size === 1) unwatch = reader.watch(() => void refresh());
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) {
          unwatch?.();
          unwatch = null;
        }
      };
    },
    refresh,
  };
}
