import { localInstanceLabel } from "../../lib/localInstanceName";
import { cleanMachineName } from "../../lib/machineIdentity";
import type { PairedPeer } from "../../lib/types";
import { jobsEntryFilter, type JobsEntry } from "./jobsLayout";
import { jobWorkspace, matchesJobsFilter, type JobsFilter, type MergedJob } from "./jobsModel";

export const THIS_MACHINE = "__this_machine__";

export function jobsEntryState(entry: JobsEntry) {
  return { machineId: THIS_MACHINE, filter: jobsEntryFilter(entry) };
}

export function jobsMachineOptions(peers: readonly PairedPeer[], localName?: string) {
  return [
    { id: THIS_MACHINE, name: cleanMachineName(localName) ?? localInstanceLabel(), online: true },
    ...peers.map(peer => ({
      id: peer.instance_id, name: cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine", online: peer.online,
    })).sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id)),
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
