import { resolveEdgeEnds } from "./edges";
import type { Box, LaidOutEdge, Point, WorkspaceLayout } from "./layout";
import type { ViewportTransform } from "./useCanvasViewport";
import type { Plan, TaskStatus } from "./types";
import { rawWorld, worldThresholds, type World } from "./worlds";

export type SceneRecordKind = "node" | "edge" | "group";

export interface SceneLodInputs {
  worldWidth: number;
  apparentWidth: number;
  thresholds: readonly [number, number, number];
  world: World;
}

export interface SceneNodeRecord {
  recordKind: "node";
  id: string;
  kind: string;
  planId: string;
  worldBox: Box;
  anchors: Record<string, Point>;
  semanticText: string;
  status: TaskStatus | Plan["status"] | null;
  lod: SceneLodInputs;
}

export interface SceneEdgeRecord {
  recordKind: "edge";
  id: string;
  kind: LaidOutEdge["kind"];
  planId: string | null;
  from: string | null;
  to: string | null;
  worldBox: Box;
  anchors: { from: Point; to: Point };
  semanticText: string;
  status: null;
  lod: SceneLodInputs;
}

export interface SceneGroupRecord {
  recordKind: "group";
  id: string;
  kind: "orchestrator-group";
  worldBox: Box;
  anchors: Record<string, Point>;
  semanticText: string;
  status: null;
  lod: SceneLodInputs;
  planIds: string[];
}

export interface CanvasScene {
  nodes: SceneNodeRecord[];
  edges: SceneEdgeRecord[];
  groups: SceneGroupRecord[];
  bounds: Box;
}

export interface CanvasSceneInput {
  layout: WorkspaceLayout;
  plans: readonly Plan[];
  cameraScale?: number;
}

export interface ProjectedSceneBox {
  id: string;
  recordKind: SceneRecordKind;
  kind: string;
  worldBox: Box;
  screenBox: Box;
}

export interface CanvasSceneReadback {
  scene: CanvasScene;
  nodeWorldBox: (id: string) => Box | null;
  nodeScreenBox: (id: string) => Box | null;
  projectedBoxes: () => ProjectedSceneBox[];
}

export interface CanvasSceneDebugRegistry {
  get: (slot?: string) => CanvasSceneReadback | null;
  list: () => string[];
}

const DEFAULT_SCENE_SLOT = "default";
const debugEntries = new Map<string, CanvasSceneReadback>();

declare global {
  interface Window {
    __codetermCanvasScene?: CanvasSceneDebugRegistry;
  }
}

function cloneBox(box: Box): Box {
  return { x: box.x, y: box.y, width: box.width, height: box.height };
}

function clonePoint(point: Point): Point {
  return { x: point.x, y: point.y };
}

function boxAnchors(box: Box): Record<string, Point> {
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  return {
    center: { x: cx, y: cy },
    top: { x: cx, y: box.y },
    right: { x: box.x + box.width, y: cy },
    bottom: { x: cx, y: box.y + box.height },
    left: { x: box.x, y: cy },
  };
}

function lodInputs(kind: string, worldWidth: number, cameraScale: number): SceneLodInputs {
  const thresholds = worldThresholds(kind);
  const apparentWidth = worldWidth * cameraScale;
  return {
    worldWidth,
    apparentWidth,
    thresholds,
    world: rawWorld(apparentWidth, thresholds),
  };
}

function edgeBox(a: Point, b: Point): Box {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) };
}

function taskStatusById(plans: readonly Plan[]): Map<string, TaskStatus> {
  const statuses = new Map<string, TaskStatus>();
  for (const plan of plans) for (const task of plan.tasks) statuses.set(task.id, task.status);
  return statuses;
}

function semanticTextForNode(kind: string, id: string, plan: Plan | undefined, plansById: Map<string, Plan>): string {
  if (kind === "plan" && plan) return plan.title;
  if (kind === "spec" && plan) return plan.primary_spec_id ? `${plan.title} spec ${plan.primary_spec_id}` : `${plan.title} spec`;
  if (kind === "task" && plan) return plan.tasks.find((task) => task.id === id)?.name ?? id;
  const element = plan?.elements.find((el) => el.id === id);
  if (element) return `${element.kind} ${element.id}`;
  const ghostId = id.startsWith("ghost:") ? id.slice(6) : id;
  return plansById.get(ghostId)?.title ?? id;
}

function edgePlanId(edge: LaidOutEdge, nodePlanById: Map<string, string>, framePlanId: string): string | null {
  if (edge.from && nodePlanById.has(edge.from)) return nodePlanById.get(edge.from)!;
  if (edge.to && nodePlanById.has(edge.to)) return nodePlanById.get(edge.to)!;
  return framePlanId || null;
}

export function projectWorldPoint(point: Point, transform: ViewportTransform): Point {
  return {
    x: point.x * transform.scale + transform.x,
    y: point.y * transform.scale + transform.y,
  };
}

export function projectWorldBox(box: Box, transform: ViewportTransform): Box {
  return {
    x: box.x * transform.scale + transform.x,
    y: box.y * transform.scale + transform.y,
    width: box.width * transform.scale,
    height: box.height * transform.scale,
  };
}

export function buildCanvasScene({ layout, plans, cameraScale = 1 }: CanvasSceneInput): CanvasScene {
  const plansById = new Map(plans.map((plan) => [plan.id, plan]));
  const statusesByTask = taskStatusById(plans);
  const nodePlanById = new Map<string, string>();
  for (const frame of layout.frames) for (const node of frame.nodes) nodePlanById.set(node.id, node.planId);

  const nodes: SceneNodeRecord[] = layout.frames.flatMap((frame) => {
    const plan = plansById.get(frame.planId);
    return frame.nodes.map((node) => ({
      recordKind: "node" as const,
      id: node.id,
      kind: node.kind,
      planId: node.planId,
      worldBox: cloneBox(node.box),
      anchors: boxAnchors(node.box),
      semanticText: semanticTextForNode(node.kind, node.id, plan, plansById),
      status: node.kind === "plan" ? plan?.status ?? null : node.kind === "task" ? statusesByTask.get(node.id) ?? null : null,
      lod: lodInputs(node.kind, node.box.width, cameraScale),
    }));
  }).sort((a, b) => a.id.localeCompare(b.id));

  const frameEdges = layout.frames.flatMap((frame) =>
    frame.edges.map((edge) => ({ edge, planId: edgePlanId(edge, nodePlanById, frame.planId) })),
  );
  const crossEdges = (layout.crossEdges ?? []).map((edge) => ({ edge, planId: edgePlanId(edge, nodePlanById, "") }));
  const nodeById = layout.nodeIndex;
  const edges: SceneEdgeRecord[] = [...frameEdges, ...crossEdges].flatMap(({ edge, planId }) => {
    const ends = resolveEdgeEnds(edge, nodeById);
    if (!ends) return [];
    return [{
      recordKind: "edge" as const,
      id: edge.id,
      kind: edge.kind,
      planId,
      from: edge.from ?? null,
      to: edge.to ?? null,
      worldBox: edgeBox(ends.a, ends.b),
      anchors: { from: clonePoint(ends.a), to: clonePoint(ends.b) },
      semanticText: `${edge.kind} ${edge.from ?? "point"} to ${edge.to ?? "point"}`,
      status: null,
      lod: lodInputs(edge.kind, Math.abs(ends.b.x - ends.a.x), cameraScale),
    }];
  }).sort((a, b) => a.id.localeCompare(b.id));

  const groups: SceneGroupRecord[] = layout.groups.map((group) => ({
    recordKind: "group" as const,
    id: group.ownerKey,
    kind: "orchestrator-group" as const,
    worldBox: cloneBox(group.box),
    anchors: boxAnchors(group.box),
    semanticText: group.title,
    status: null,
    lod: lodInputs("group", group.box.width, cameraScale),
    planIds: [...group.planIds].sort(),
  })).sort((a, b) => a.id.localeCompare(b.id));

  return { nodes, edges, groups, bounds: cloneBox(layout.bounds) };
}

export function projectSceneBoxes(scene: CanvasScene, transform: ViewportTransform): ProjectedSceneBox[] {
  const nodes = scene.nodes.map((node) => ({
    id: node.id,
    recordKind: node.recordKind,
    kind: node.kind,
    worldBox: cloneBox(node.worldBox),
    screenBox: projectWorldBox(node.worldBox, transform),
  }));
  const groups = scene.groups.map((group) => ({
    id: group.id,
    recordKind: group.recordKind,
    kind: group.kind,
    worldBox: cloneBox(group.worldBox),
    screenBox: projectWorldBox(group.worldBox, transform),
  }));
  return [...nodes, ...groups].sort((a, b) => `${a.recordKind}:${a.id}`.localeCompare(`${b.recordKind}:${b.id}`));
}

export function createCanvasSceneReadback(scene: CanvasScene, transform: ViewportTransform): CanvasSceneReadback {
  const worldBoxes = new Map(scene.nodes.map((node) => [node.id, node.worldBox]));
  return {
    scene,
    nodeWorldBox: (id) => {
      const box = worldBoxes.get(id);
      return box ? cloneBox(box) : null;
    },
    nodeScreenBox: (id) => {
      const box = worldBoxes.get(id);
      return box ? projectWorldBox(box, transform) : null;
    },
    projectedBoxes: () => projectSceneBoxes(scene, transform),
  };
}

export function publishCanvasSceneReadback(
  scene: CanvasScene,
  transform: ViewportTransform,
  slot = DEFAULT_SCENE_SLOT,
): () => void {
  if (typeof window === "undefined") return () => {};
  const readback = createCanvasSceneReadback(scene, transform);
  debugEntries.set(slot, readback);
  window.__codetermCanvasScene = {
    get: (id = DEFAULT_SCENE_SLOT) => debugEntries.get(id) ?? null,
    list: () => [...debugEntries.keys()].sort(),
  };
  return () => {
    if (debugEntries.get(slot) === readback) debugEntries.delete(slot);
  };
}
