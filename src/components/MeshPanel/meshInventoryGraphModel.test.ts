import type { Service } from "./types";
import {
  buildMeshGraphLayout,
  buildMeshInventoryGraphModel,
  machineNodeId,
  resourceNodeId,
} from "./meshInventoryGraphModel";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function service(overrides: Partial<Service> & Pick<Service, "id" | "name" | "kind">): Service {
  return {
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    ...overrides,
    id: overrides.id,
    name: overrides.name,
    kind: overrides.kind,
  };
}

const resources = [
  service({ id: "cron-backup", name: "Backup", kind: "cron", machine_id: "machine-b", machine_name: "Remote", provenance: { freshness: "stale", offline: false, partial_error: "old schedule" } }),
  service({ id: "svc-api", name: "API", kind: "daemon", machine_id: "machine-a", machine_name: "Local" }),
  service({ id: "svc-cache", name: "Cache", kind: "daemon", machine_id: "machine-b", machine_name: "Remote", peer_online: false, provenance: { freshness: "offline", offline: true }, capabilities: { get: { allowed: false, unavailable_reason: "not_authorized" } } }),
];

const model = buildMeshInventoryGraphModel({
  resources,
  peerIssues: [{ machineId: "machine-c", machineName: "Failed", freshness: "offline", partialError: "peer did not respond" }],
});

assert(model.machines.map((node) => node.id).join(",") === "machine:machine-a,machine:machine-b,machine:machine-c", "machines have deterministic ids and ordering");
assert(model.resources.map((node) => node.id).join(",") === "resource:machine-a:svc-api,resource:machine-b:cron-backup,resource:machine-b:svc-cache", "resources have deterministic qualified ids and ordering");
assert(model.resources[0]?.type === "service" && model.resources[1]?.type === "schedule", "daemon and cron resources have distinct graph types");
assert(model.edges.every((edge) => edge.type === "ownership" && edge.source.startsWith("machine:") && edge.target.startsWith("resource:")), "graph edges only connect machines to resources");
assert(model.edges.length === resources.length, "each resource has one ownership edge");
assert(model.resources.find((node) => node.label === "Backup")?.status.freshness === "stale", "stale resource status survives projection");
assert(model.resources.find((node) => node.label === "Cache")?.status.freshness === "offline", "offline resource status survives projection");
assert(model.resources.find((node) => node.label === "Cache")?.status.unavailableReason === "not_authorized", "denied capability survives projection");
assert(model.machines.find((node) => node.machineId === "machine-c")?.status.error === "peer did not respond", "partial peer failure remains visible as a machine node");
assert(machineNodeId("machine-a") === "machine:machine-a" && resourceNodeId("machine-a:svc-api") === "resource:machine-a:svc-api", "node id helpers preserve identity");

const denseResources = Array.from({ length: 12 }, (_, index) => service({
  id: `dense-${index}`,
  name: `Dense ${index}`,
  kind: index % 2 === 0 ? "daemon" : "cron",
  machine_id: index < 6 ? "machine-a" : "machine-b",
  machine_name: index < 6 ? "Local" : "Remote",
}));
const denseModel = buildMeshInventoryGraphModel({ resources: denseResources });
const denseLayout = buildMeshGraphLayout(denseModel, 720, 360);
for (const machine of denseModel.machines) {
  const owned = denseModel.resources
    .filter((resource) => resource.machineId === machine.machineId)
    .map((resource) => denseLayout.points.get(resource.id)!)
    .sort((left, right) => left.y - right.y);
  assert(owned.every((point, index) => index === 0 || point.y - owned[index - 1].y >= 80), `dense ${machine.machineId} resources do not overlap`);
  assert(owned.every((point) => point.y - 32 >= 0 && point.y + 32 <= denseLayout.height), `dense ${machine.machineId} resources stay within layout bounds`);
}
assert(denseLayout.height > 360, "dense layout grows its content height instead of compressing nodes");

const mixedCapabilities = service({
  id: "mixed-capabilities",
  name: "Mixed",
  kind: "daemon",
  capabilities: {
    get: { allowed: true },
    delete: { allowed: false, unavailable_reason: "unsupported_resource_kind" },
  },
});
const concreteReason = service({
  id: "concrete-reason",
  name: "Denied",
  kind: "daemon",
  capabilities: {
    get: { allowed: false, unavailable_reason: "unsupported_resource_kind" },
    delete: { allowed: false, unavailable_reason: "not_authorized" },
  },
});
const capabilityModel = buildMeshInventoryGraphModel({ resources: [mixedCapabilities, concreteReason] });
assert(capabilityModel.resources.find((node) => node.label === "Mixed")?.status.unavailableReason === undefined, "allowed capability prevents denied labeling");
assert(capabilityModel.resources.find((node) => node.label === "Denied")?.status.unavailableReason === "not_authorized", "concrete denial reason outranks unsupported kind");

console.log("✓ Mesh graph model preserves deterministic ownership and status semantics");
