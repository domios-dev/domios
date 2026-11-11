export type ServiceKind = import("../../../gen/ServiceKind").ServiceKind;
export type HealthMode = import("../../../gen/HealthMode").HealthMode;
export type HealthStatus = import("../../../gen/HealthStatus").HealthStatus;
export type ActivitySource = import("../../../gen/ActivitySource").ActivitySource;
export type ActivityLevel = import("../../../gen/ActivityLevel").ActivityLevel;
export type InventoryOperation = import("../../../gen/InventoryOperation").InventoryOperation;
export type InventoryUnavailableReason = import("../../../gen/InventoryUnavailableReason").InventoryUnavailableReason;

export type InventoryCapability = import("../../../gen/InventoryCapability").InventoryCapability;

export type InventoryProvenance = Omit<import("../../../gen/InventoryProvenance").InventoryProvenance, "fetched_at"> & Partial<Pick<import("../../../gen/InventoryProvenance").InventoryProvenance, "fetched_at">>;

export type HealthSnapshot = Pick<import("../../../gen/HealthSnapshot").HealthSnapshot, "status"> & Partial<Omit<import("../../../gen/HealthSnapshot").HealthSnapshot, "status">>;

export type Service = Omit<import("../../../gen/Service").Service, "health" | "protected" | "health_ttl"> & Partial<Pick<import("../../../gen/Service").Service, "protected" | "health_ttl">> & {
health: HealthSnapshot;
source_peer_id?: string;
source_peer?: string;
stale?: boolean;
peer_online?: boolean;
resource_id?: string;
machine_id?: string;
machine_name?: string;
provenance?: InventoryProvenance;
capabilities?: Partial<Record<InventoryOperation, InventoryCapability>>;
};

export type ActivityEvent = import("../../../gen/ActivityEvent").ActivityEvent;

export type MeshTab = "inventory" | "activity" | "network";

export const CRON_ID_PREFIX = "cron_";

export function serviceIdToCronId(serviceId: string): string {
  return serviceId.startsWith(CRON_ID_PREFIX)
    ? serviceId.slice(CRON_ID_PREFIX.length)
    : serviceId;
}
