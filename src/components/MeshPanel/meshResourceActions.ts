import type { CronJob, CronRunInfo } from "../../lib/types";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { OutcomeUnknown } from "../../lib/streamMux";
import { InvalidJsonResponseError, TransportError } from "../../lib/transportClient";
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
  const id = service.source_peer_id ?? service.machine_id ?? "local";
  return {
    id,
    name: service.machine_name ?? service.source_peer ?? localMachineName,
    remote: Boolean(service.source_peer_id ?? service.source_peer),
  };
}

export function capabilityFor(service: Service, operation: InventoryOperation): InventoryCapability {
  const capability = service.capabilities?.[operation];
  if (capability) return capability;
  // Absent map = not projected yet (loading), not a backend "unsupported" verdict.
  if (!service.capabilities) return { allowed: false };
  return { allowed: false, unavailable_reason: "unsupported_resource_kind" };
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

export function createMeshResourceActionAdapter(transportForPeer = daemonTransportForPeer): MeshResourceActionAdapter {
  const client = (service: Service) => {
    if (service.source_peer && !service.source_peer_id) throw new Error("Remote resource requires an installation ID");
    return transportForPeer(service.source_peer_id);
  };
  const resourceId = (service: Service) => service.resource_id ?? service.id;
  const cronPath = (service: Service) => `/api/crons/${encodeURIComponent(serviceIdToCronId(resourceId(service)))}`;
  const servicePath = (service: Service) => `/api/services/${encodeURIComponent(resourceId(service))}`;

  return {
    get: async (service) => client(service).json("GET", cronPath(service)),
    history: async (service) => client(service).json("GET", `${cronPath(service)}/history`),
    trigger: async (service) => client(service).json("POST", `${cronPath(service)}/trigger`),
    setEnabled: async (service, enabled) => client(service).json("POST", `${cronPath(service)}/${enabled ? "enable" : "disable"}`),
    cancel: async (service) => client(service).json("POST", `${cronPath(service)}/cancel`),
    updateSchedule: async (service, schedule) => client(service).json(service.source_peer_id ? "PUT" : "PATCH", cronPath(service), { schedule }),
    remove: async (service) => client(service).json("DELETE", service.kind === "cron" ? cronPath(service) : servicePath(service)),
    regenerateToken: async (service) => {
      if (resourceTarget(service, "").remote) throw new Error("Push tokens can only be rotated locally");
      const result = await client(service).json<{ token: string }>("POST", `${servicePath(service)}/regenerate-token`);
      return result.token;
    },
  };
}

export function isUnconfirmedMutation(error: unknown): boolean {
  if (error instanceof OutcomeUnknown || error instanceof InvalidJsonResponseError) return true;
  if (!(error instanceof TransportError)) return false;
  try {
    const { code } = JSON.parse(error.body)?.error ?? {};
    return code === "mesh_transport_submit_unconfirmed" || code === "relay_delivery_deadline_exceeded";
  } catch {
    return false;
  }
}

export function mutationFailure(action: string, target: MeshResourceTarget, error: unknown): string {
  if (isUnconfirmedMutation(error)) return `${action} outcome is unconfirmed on ${target.name}.`;
  const detail = error instanceof Error ? error.message : String(error);
  return `${action} failed: target machine ${target.name} rejected or failed the request${detail ? ` (${detail})` : ""}.`;
}

export function unavailableFailure(action: string, reason: string | undefined): string {
  return `${action} unavailable: no request was allowed or sent${reason ? ` (${reason})` : ""}.`;
}
