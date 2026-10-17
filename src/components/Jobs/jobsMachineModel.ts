import type { PairedPeer } from "../../lib/types";
import { jobsEntryFilter, type JobsEntry } from "./jobsLayout";
import { jobWorkspace, matchesJobsFilter, type JobsFilter, type MergedJob } from "./jobsModel";

export const THIS_MACHINE = "__this_machine__";

export function jobsEntryState(entry: JobsEntry) {
  return { machineId: THIS_MACHINE, filter: jobsEntryFilter(entry) };
}

export function jobsMachineOptions(peers: readonly PairedPeer[]) {
  return [
    { id: THIS_MACHINE, name: "This machine", online: true },
    ...[...peers].sort((a, b) => a.instance_name.localeCompare(b.instance_name) || a.instance_id.localeCompare(b.instance_id))
      .map((peer) => ({ id: peer.instance_id, name: peer.instance_name, online: peer.online })),
  ];
}

export function jobsMachineAvailability(machineId: string, peers: readonly PairedPeer[]) {
  if (machineId === THIS_MACHINE) return "local";
  const peer = peers.find((candidate) => candidate.instance_id === machineId);
  return peer?.online ? "online" : "unavailable";
}

export function filterMachineJobs(jobs: readonly MergedJob[], filter: JobsFilter) {
  return jobs.filter((job) => matchesJobsFilter(filter, jobWorkspace(job), job.kind === "cron"
    ? job.cron.team_id : job.kind === "blueprint" ? job.blueprint.identity.team_id : job.sentinel.scenario.root.team_id));
}

export function peerJobsState(data: { partial: boolean } | null, failed: boolean, visibleCount: number) {
  if (failed) return "unavailable";
  if (!data) return "loading";
  if (data.partial) return "partial";
  return visibleCount === 0 ? "empty" : "ready";
}
