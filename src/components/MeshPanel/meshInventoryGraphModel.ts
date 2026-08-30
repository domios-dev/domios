import {
  buildInventoryModel,
  inventoryResourceKey,
  type InventoryPeerIssue,
  type InventorySnapshot,
  type InventoryViewState,
} from "./meshInventoryModel";
import type { Service } from "./types";

export type MeshGraphNodeType = "machine" | "service" | "schedule";
export type MeshGraphFreshness = "fresh" | "stale" | "offline";

export interface MeshGraphNodeStatus {
  freshness: MeshGraphFreshness;
  state?: string;
  unavailableReason?: string;
  error?: string;
}

export interface MeshGraphMachineNode {
  id: string;
  type: "machine";
  label: string;
  machineId: string;
  status: MeshGraphNodeStatus;
  resourceCount: number;
}

export interface MeshGraphResourceNode {
  id: string;
  type: "service" | "schedule";
  label: string;
  resource: Service;
  resourceKey: string;
  machineId: string;
  machineName: string;
  status: MeshGraphNodeStatus;
}

export type MeshGraphNode = MeshGraphMachineNode | MeshGraphResourceNode;

export interface MeshGraphEdge {
  id: string;
  source: string;
  target: string;
  type: "ownership";
}

export interface MeshInventoryGraphModel {
  nodes: MeshGraphNode[];
  edges: MeshGraphEdge[];
  machines: MeshGraphMachineNode[];
  resources: MeshGraphResourceNode[];
  state: InventoryViewState;
  totalResources: number;
  peerIssues: InventoryPeerIssue[];
}

export interface MeshGraphPoint {
  x: number;
  y: number;
}

export interface MeshGraphLayout {
  width: number;
  height: number;
  points: Map<string, MeshGraphPoint>;
}

const RESOURCE_NODE_HEIGHT = 64;
const RESOURCE_GAP = 16;
const MACHINE_BAND_GAP = 32;
const LAYOUT_PADDING = 32;
const MIN_WIDTH = 720;
const MIN_HEIGHT = 360;

export function machineNodeId(machineId: string): string {
  return `machine:${machineId}`;
}

export function resourceNodeId(resourceKey: string): string {
  return `resource:${resourceKey}`;
}

function unavailableReason(resource: Service): string | undefined {
  const capabilities = Object.values(resource.capabilities ?? {});
  if (capabilities.some((capability) => capability.allowed)) return undefined;
  const denied = capabilities.filter((capability) => !capability.allowed);
  if (denied.length === 0) return undefined;
  const reasons = denied
    .map((capability) => capability.unavailable_reason)
    .filter((reason): reason is string => Boolean(reason));
  const concreteReasons = reasons.filter((reason) => reason !== "unsupported_resource_kind");
  return [...new Set(concreteReasons.length ? concreteReasons : reasons)].join(", ") || "not_authorized";
}

function serviceStatus(resource: Service, freshness: MeshGraphFreshness): MeshGraphNodeStatus {
  return {
    freshness,
    state: resource.kind === "cron"
      ? resource.enabled ? resource.last_run_status ?? "idle" : "disabled"
      : resource.health.status,
    unavailableReason: unavailableReason(resource),
    error: resource.provenance?.partial_error,
  };
}

function machineStatus(
  machineId: string,
  rows: ReturnType<typeof buildInventoryModel>["allRows"],
  issues: InventoryPeerIssue[],
): MeshGraphNodeStatus {
  const issue = issues.find((candidate) => candidate.machineId === machineId);
  const machineRows = rows.filter((candidate) => candidate.machineId === machineId);
  const freshness = machineRows.reduce<MeshGraphFreshness>((current, candidate) => {
    const rank: Record<MeshGraphFreshness, number> = { fresh: 0, stale: 1, offline: 2 };
    return rank[candidate.freshness] > rank[current] ? candidate.freshness : current;
  }, "fresh");
  return {
    freshness: issue?.freshness ?? freshness,
    error: issue?.partialError ?? undefined,
  };
}

export function buildMeshInventoryGraphModel(
  snapshot: InventorySnapshot,
  thisMachineName = "This machine",
): MeshInventoryGraphModel {
  const inventory = buildInventoryModel(snapshot, undefined, undefined, thisMachineName);
  const machineMap = new Map<string, MeshGraphMachineNode>();
  const resourceNodes: MeshGraphResourceNode[] = [];

  for (const row of inventory.allRows) {
    if (!machineMap.has(row.machineId)) {
      machineMap.set(row.machineId, {
        id: machineNodeId(row.machineId),
        type: "machine",
        label: row.machineName,
        machineId: row.machineId,
        status: machineStatus(row.machineId, inventory.allRows, inventory.peerIssues),
        resourceCount: 0,
      });
    }
    const machine = machineMap.get(row.machineId)!;
    machine.resourceCount += 1;
    resourceNodes.push({
      id: resourceNodeId(row.resourceKey),
      type: row.kind === "cron" ? "schedule" : "service",
      label: row.resource.name,
      resource: row.resource,
      resourceKey: row.resourceKey,
      machineId: row.machineId,
      machineName: row.machineName,
      status: serviceStatus(row.resource, row.freshness),
    });
  }

  for (const issue of inventory.peerIssues) {
    if (machineMap.has(issue.machineId)) continue;
    machineMap.set(issue.machineId, {
      id: machineNodeId(issue.machineId),
      type: "machine",
      label: issue.machineName,
      machineId: issue.machineId,
      status: { freshness: issue.freshness, error: issue.partialError ?? undefined },
      resourceCount: 0,
    });
  }

  const machines = [...machineMap.values()].sort((left, right) => left.label.localeCompare(right.label) || left.machineId.localeCompare(right.machineId));
  resourceNodes.sort((left, right) => left.machineId.localeCompare(right.machineId) || left.type.localeCompare(right.type) || left.label.localeCompare(right.label) || left.resourceKey.localeCompare(right.resourceKey));
  const edges = resourceNodes.map((resource) => ({
    id: `ownership:${resource.machineId}:${resource.resourceKey}`,
    source: machineNodeId(resource.machineId),
    target: resource.id,
    type: "ownership" as const,
  }));

  return {
    nodes: [...machines, ...resourceNodes],
    edges,
    machines,
    resources: resourceNodes,
    state: inventory.state,
    totalResources: resourceNodes.length,
    peerIssues: inventory.peerIssues,
  };
}

export function buildMeshGraphLayout(model: MeshInventoryGraphModel, width: number, height: number): MeshGraphLayout {
  const safeWidth = Math.max(width, MIN_WIDTH);
  const safeHeight = Math.max(height, MIN_HEIGHT);
  const machineColumnX = Math.max(120, Math.min(210, safeWidth * 0.22));
  const resourceColumnX = Math.min(safeWidth - 180, Math.max(500, safeWidth * 0.7));
  const points = new Map<string, MeshGraphPoint>();
  let bandTop = LAYOUT_PADDING;

  model.machines.forEach((machine) => {
    const owned = model.resources.filter((resource) => resource.machineId === machine.machineId);
    const bandHeight = owned.length > 0
      ? owned.length * RESOURCE_NODE_HEIGHT + (owned.length - 1) * RESOURCE_GAP + 24
      : RESOURCE_NODE_HEIGHT + 24;
    const machineY = bandTop + bandHeight / 2;
    points.set(machine.id, { x: machineColumnX, y: machineY });
    const resourceStep = RESOURCE_NODE_HEIGHT + RESOURCE_GAP;
    owned.forEach((resource, resourceIndex) => {
      points.set(resource.id, {
        x: resourceColumnX,
        y: bandTop + 12 + RESOURCE_NODE_HEIGHT / 2 + resourceIndex * resourceStep,
      });
    });
    bandTop += bandHeight + MACHINE_BAND_GAP;
  });

  return { width: safeWidth, height: Math.max(safeHeight, bandTop - MACHINE_BAND_GAP + LAYOUT_PADDING), points };
}

export function graphResourceKey(resource: Service): string {
  return inventoryResourceKey(resource);
}
