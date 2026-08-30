import type { CronJob, CronRunInfo } from "../../lib/types";
import {
  cronAction,
  cronDelete,
  cronGet,
  cronHistory,
  cronUpdate,
  peerCronAction,
  peerCronDelete,
  peerCronGet,
  peerCronHistory,
  peerCronUpdate,
  peerServiceDelete,
  servicesDelete,
  servicesRegenerateToken,
} from "./meshApi";
import { serviceIdToCronId, type InventoryCapability, type InventoryOperation, type Service } from "./types";

export interface MeshResourceTarget {
  id: string;
  name: string;
  remote: boolean;
}

export interface MeshResourceActionAdapter {
  get(service: Service): Promise<CronJob>;
  history(service: Service): Promise<CronRunInfo[]>;
  trigger(service: Service): Promise<unknown>;
  setEnabled(service: Service, enabled: boolean): Promise<unknown>;
  cancel(service: Service): Promise<unknown>;
  updateSchedule(service: Service, schedule: string): Promise<unknown>;
  remove(service: Service): Promise<unknown>;
  regenerateToken(service: Service): Promise<string>;
}

export const OPERATION_LABELS: Record<InventoryOperation, string> = {
  get: "inspect",
  history: "history",
  update_schedule: "edit schedule",
  delete: "remove",
  enable: "resume",
  disable: "pause",
  trigger: "run now",
  cancel: "cancel running",
};

export function resourceTarget(service: Service, localMachineName: string): MeshResourceTarget {
  const id = service.machine_id ?? service.source_peer_id ?? service.source_peer ?? "local";
  return {
    id,
    name: service.machine_name ?? service.source_peer ?? localMachineName,
    remote: Boolean(service.source_peer_id ?? service.source_peer),
  };
}

export function capabilityFor(service: Service, operation: InventoryOperation): InventoryCapability {
  return service.capabilities?.[operation] ?? { allowed: false, unavailable_reason: "unsupported_resource_kind" };
}

export function capabilityReason(service: Service, operation: InventoryOperation): string | undefined {
  return capabilityFor(service, operation).unavailable_reason;
}

export function availableOperations(service: Service): InventoryOperation[] {
  return (Object.entries(service.capabilities ?? {}) as [InventoryOperation, InventoryCapability][])
    .filter(([, capability]) => capability.allowed)
    .map(([operation]) => operation)
    .sort();
}

export function capabilitySummary(service: Service): string {
  const available = availableOperations(service);
  return available.length === 0
    ? "No operations available"
    : available.map((operation) => OPERATION_LABELS[operation]).join(", ");
}

export function createMeshResourceActionAdapter(): MeshResourceActionAdapter {
  const peerId = (service: Service): string | null => service.source_peer_id ?? service.source_peer ?? null;
  const cronId = (service: Service): string => serviceIdToCronId(service.id);

  return {
    get: (service) => {
      const instanceId = peerId(service);
      return instanceId ? peerCronGet(instanceId, cronId(service)) : cronGet(cronId(service));
    },
    history: (service) => {
      const instanceId = peerId(service);
      return instanceId ? peerCronHistory(instanceId, cronId(service)) : cronHistory(cronId(service));
    },
    trigger: (service) => {
      const instanceId = peerId(service);
      return instanceId ? peerCronAction(instanceId, cronId(service), "trigger") : cronAction(cronId(service), "trigger");
    },
    setEnabled: (service, enabled) => {
      const instanceId = peerId(service);
      const action = enabled ? "enable" : "disable";
      return instanceId ? peerCronAction(instanceId, cronId(service), action) : cronAction(cronId(service), action);
    },
    cancel: (service) => {
      const instanceId = peerId(service);
      return instanceId ? peerCronAction(instanceId, cronId(service), "cancel") : cronAction(cronId(service), "cancel");
    },
    updateSchedule: (service, schedule) => {
      const instanceId = peerId(service);
      const patch = { schedule };
      return instanceId ? peerCronUpdate(instanceId, cronId(service), patch) : cronUpdate(cronId(service), patch);
    },
    remove: (service) => {
      const instanceId = peerId(service);
      if (instanceId) {
        return service.kind === "cron"
          ? peerCronDelete(instanceId, cronId(service))
          : peerServiceDelete(instanceId, service.id);
      }
      return service.kind === "cron" ? cronDelete(cronId(service)) : servicesDelete(service.id);
    },
    regenerateToken: (service) => servicesRegenerateToken(service.id),
  };
}

export function mutationFailure(action: string, target: MeshResourceTarget, error: unknown): string {
  const detail = error instanceof Error ? error.message : String(error);
  return `${action} failed: target machine ${target.name} rejected or failed the request${detail ? ` (${detail})` : ""}.`;
}

export function unavailableFailure(action: string, reason: string | undefined): string {
  return `${action} unavailable: no request was allowed or sent${reason ? ` (${reason})` : ""}.`;
}
