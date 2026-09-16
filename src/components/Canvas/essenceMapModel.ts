export const ESSENCE_MAP_MAX_NODES = 100;

export type EssenceMapNodeType = "vector" | "capability" | "core" | "client";
export type EssenceMapEdgeType = "runs_on" | "feeds" | "part_of" | "client_of";

export interface EssenceMapSource {
  path: string;
  anchor: string;
  sha256: string;
}

export interface EssenceMapNode {
  kind: "node";
  id: string;
  type: EssenceMapNodeType;
  label: string;
  vector: string | null;
  source: EssenceMapSource;
  runs_on_reason?: string;
}

export interface EssenceMapEdge {
  kind: "edge";
  from: string;
  to: string;
  type: EssenceMapEdgeType;
  source: EssenceMapSource;
}

export type EssenceMapRecord = EssenceMapNode | EssenceMapEdge;

export interface EssenceMapFile {
  project_path: string;
  map_path: string;
  status: "missing" | "empty" | "ready" | "oversized" | "unreadable";
  content?: string | null;
  error?: string | null;
}

export type EssenceMapProjectStatus = "valid" | "empty" | "malformed" | "oversized" | "missing" | "unreadable";

export interface EssenceMapProject {
  file: EssenceMapFile;
  status: EssenceMapProjectStatus;
  nodes: EssenceMapNode[];
  edges: EssenceMapEdge[];
  error?: string;
}

const NODE_TYPES: readonly EssenceMapNodeType[] = ["vector", "capability", "core", "client"];
const EDGE_TYPES: readonly EssenceMapEdgeType[] = ["runs_on", "feeds", "part_of", "client_of"];

function recordObject(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function stringValue(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function sourceValue(value: unknown): value is EssenceMapSource {
  const source = recordObject(value);
  return source !== null
    && stringValue(source.path)
    && stringValue(source.anchor)
    && stringValue(source.sha256);
}

function nodeValue(value: unknown): value is EssenceMapNode {
  const node = recordObject(value);
  return node !== null
    && node.kind === "node"
    && stringValue(node.id)
    && NODE_TYPES.includes(node.type as EssenceMapNodeType)
    && stringValue(node.label)
    && (node.vector === null || stringValue(node.vector))
    && sourceValue(node.source)
    && (node.runs_on_reason === undefined || stringValue(node.runs_on_reason));
}

function edgeValue(value: unknown): value is EssenceMapEdge {
  const edge = recordObject(value);
  return edge !== null
    && edge.kind === "edge"
    && stringValue(edge.from)
    && stringValue(edge.to)
    && EDGE_TYPES.includes(edge.type as EssenceMapEdgeType)
    && sourceValue(edge.source);
}

export type EssenceMapParseResult =
  | { status: "empty"; nodes: []; edges: [] }
  | { status: "valid"; nodes: EssenceMapNode[]; edges: EssenceMapEdge[] }
  | { status: "malformed" | "oversized"; nodes: []; edges: []; error: string };

export function parseEssenceMapJsonl(content: string): EssenceMapParseResult {
  if (!content.trim()) return { status: "empty", nodes: [], edges: [] };
  const values: unknown[] = [];
  for (const [index, line] of content.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    try {
      values.push(JSON.parse(line));
    } catch {
      return { status: "malformed", nodes: [], edges: [], error: `Invalid JSON on line ${index + 1}` };
    }
  }
  if (values.length === 0) return { status: "empty", nodes: [], edges: [] };

  const nodes: EssenceMapNode[] = [];
  const edges: EssenceMapEdge[] = [];
  const nodeIds = new Set<string>();
  for (const value of values) {
    if (nodeValue(value)) {
      if (nodeIds.has(value.id)) {
        return { status: "malformed", nodes: [], edges: [], error: `Duplicate node ${value.id}` };
      }
      nodeIds.add(value.id);
      nodes.push(value);
    } else if (edgeValue(value)) {
      edges.push(value);
    } else {
      return { status: "malformed", nodes: [], edges: [], error: "A map record is not a valid source-backed node or edge" };
    }
  }
  if (nodes.length > ESSENCE_MAP_MAX_NODES) {
    return {
      status: "oversized",
      nodes: [],
      edges: [],
      error: `Map contains ${nodes.length} nodes; the per-project limit is ${ESSENCE_MAP_MAX_NODES}`,
    };
  }
  if (edges.some((edge) => !nodeIds.has(edge.from) || !nodeIds.has(edge.to))) {
    return { status: "malformed", nodes: [], edges: [], error: "A map edge references a node that is not present" };
  }
  return { status: "valid", nodes, edges };
}

export function parseEssenceMapProject(file: EssenceMapFile): EssenceMapProject {
  if (file.status === "missing") return { file, status: "missing", nodes: [], edges: [] };
  if (file.status === "empty") return { file, status: "empty", nodes: [], edges: [] };
  if (file.status === "oversized") {
    return { file, status: "oversized", nodes: [], edges: [], error: file.error ?? "Map is oversized" };
  }
  if (file.status === "unreadable") {
    return { file, status: "unreadable", nodes: [], edges: [], error: file.error ?? "Map could not be read" };
  }
  const parsed = parseEssenceMapJsonl(file.content ?? "");
  if (parsed.status === "valid") {
    return { file, status: "valid", nodes: parsed.nodes, edges: parsed.edges };
  }
  if (parsed.status === "empty") return { file, status: "empty", nodes: [], edges: [] };
  return { file, status: parsed.status, nodes: [], edges: [], error: parsed.error };
}

export function projectPathsForWorkspace(
  workspace: { cwd: string | null; tabs: string[] },
  tabs: Record<string, { cwd: string | null }>,
): string[] {
  const paths: string[] = [];
  const seen = new Set<string>();
  const add = (path: string | null | undefined) => {
    const value = path?.trim();
    if (!value) return;
    const key = value.replace(/[\\/]+$/, "").replaceAll("\\", "/");
    if (seen.has(key)) return;
    seen.add(key);
    paths.push(value);
  };
  add(workspace.cwd);
  for (const tabId of workspace.tabs) add(tabs[tabId]?.cwd);
  return paths;
}

export interface EssenceMapNodeLayout {
  node: EssenceMapNode;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EssenceMapClusterLayout {
  vectorId: string | null;
  vector: EssenceMapNode | null;
  colorIndex: number;
  x: number;
  y: number;
  width: number;
  height: number;
  nodeIds: string[];
}

export interface EssenceMapRegionLayout {
  type: "core" | "client";
  label: string;
  x: number;
  y: number;
  width: number;
  height: number;
  nodeIds: string[];
}

export interface EssenceMapProjectLayout {
  width: number;
  height: number;
  nodes: EssenceMapNodeLayout[];
  nodeById: Map<string, EssenceMapNodeLayout>;
  clusters: EssenceMapClusterLayout[];
  regions: EssenceMapRegionLayout[];
}

export interface EssenceMapWorldProject extends EssenceMapProject {
  layout: EssenceMapProjectLayout;
  x: number;
  y: number;
}

export interface EssenceMapWorldLayout {
  width: number;
  height: number;
  projects: EssenceMapWorldProject[];
  projectBounds: Map<string, { x: number; y: number; width: number; height: number }>;
}

const NODE_HEIGHT = 84;
const NODE_GAP = 12;
const SECTION_GAP = 32;
const CLUSTER_GAP = 16;
const CANVAS_PADDING = 18;
const CLUSTER_PADDING = 12;
const CLUSTER_HEADER_HEIGHT = 26;
const REGION_HEADER_HEIGHT = 28;
const REGION_PADDING = 14;

function layoutColumns(width: number): number {
  if (width >= 1080) return 7;
  if (width >= 800) return 4;
  if (width >= 560) return 3;
  if (width >= 460) return 2;
  return 1;
}

function sortedNodes(nodes: EssenceMapNode[]): EssenceMapNode[] {
  return [...nodes].sort((a, b) => a.id.localeCompare(b.id));
}

function gridHeight(count: number, columns: number): number {
  if (count === 0) return 0;
  const rows = Math.ceil(count / columns);
  return rows * NODE_HEIGHT + Math.max(0, rows - 1) * NODE_GAP;
}

function gridX(index: number, columns: number, cardWidth: number, gap: number, width: number): number {
  const contentWidth = columns * cardWidth + Math.max(0, columns - 1) * gap;
  return Math.floor((width - contentWidth) / 2) + index * (cardWidth + gap);
}

function edgeSignature(nodeId: string, edges: EssenceMapEdge[]): string {
  return edges
    .filter((edge) => edge.from === nodeId)
    .map((edge) => `${edge.type}:${edge.to}`)
    .sort()
    .join("|");
}

function orderedCapabilities(
  nodes: EssenceMapNode[],
  edges: EssenceMapEdge[],
  targetIndexes: Map<string, number>,
): EssenceMapNode[] {
  return [...nodes].sort((a, b) => {
    const aTargets = edges.filter((edge) => edge.from === a.id).map((edge) => targetIndexes.get(edge.to) ?? Number.POSITIVE_INFINITY);
    const bTargets = edges.filter((edge) => edge.from === b.id).map((edge) => targetIndexes.get(edge.to) ?? Number.POSITIVE_INFINITY);
    const aScore = aTargets.length === 0 ? Number.POSITIVE_INFINITY : aTargets.reduce((sum, index) => sum + index, 0) / aTargets.length;
    const bScore = bTargets.length === 0 ? Number.POSITIVE_INFINITY : bTargets.reduce((sum, index) => sum + index, 0) / bTargets.length;
    const scoreOrder = aScore - bScore;
    const signatureOrder = edgeSignature(a.id, edges).localeCompare(edgeSignature(b.id, edges));
    return scoreOrder || signatureOrder || a.id.localeCompare(b.id);
  });
}

function orderedCoreNodes(
  nodes: EssenceMapNode[],
  edges: EssenceMapEdge[],
  vectorIndexes: Map<string, number>,
): EssenceMapNode[] {
  const scores = new Map<string, number>();
  for (const core of nodes) {
    const incoming = edges
      .filter((edge) => edge.to === core.id)
      .map((edge) => vectorIndexes.get(edge.from) ?? -1)
      .filter((index) => index >= 0);
    scores.set(core.id, incoming.length === 0 ? Number.POSITIVE_INFINITY : incoming.reduce((sum, index) => sum + index, 0) / incoming.length);
  }
  return [...nodes].sort((a, b) => (scores.get(a.id)! - scores.get(b.id)!) || a.id.localeCompare(b.id));
}

function placeNode(
  layouts: EssenceMapNodeLayout[],
  node: EssenceMapNode,
  x: number,
  y: number,
  width: number,
): EssenceMapNodeLayout {
  const layout = { node, x, y, width, height: NODE_HEIGHT };
  layouts.push(layout);
  return layout;
}

function placeRegion(
  layouts: EssenceMapNodeLayout[],
  type: "core" | "client",
  nodes: EssenceMapNode[],
  y: number,
  contentWidth: number,
  columns: number,
): EssenceMapRegionLayout {
  const cardWidth = Math.floor((contentWidth - Math.max(0, columns - 1) * CLUSTER_GAP) / columns);
  const contentY = y + REGION_HEADER_HEIGHT;
  for (const [index, node] of nodes.entries()) {
    const column = index % columns;
    const row = Math.floor(index / columns);
    placeNode(layouts, node, CANVAS_PADDING + gridX(column, columns, cardWidth, CLUSTER_GAP, contentWidth), contentY + row * (NODE_HEIGHT + NODE_GAP), cardWidth);
  }
  const height = REGION_HEADER_HEIGHT + gridHeight(nodes.length, columns) + REGION_PADDING;
  return {
    type,
    label: type === "core" ? "Core parts" : "Clients",
    x: CANVAS_PADDING,
    y,
    width: contentWidth,
    height,
    nodeIds: nodes.map((node) => node.id),
  };
}

export function layoutEssenceMapProject(
  nodes: EssenceMapNode[],
  availableWidth: number,
  edges: EssenceMapEdge[] = [],
): EssenceMapProjectLayout {
  const width = Math.max(280, Math.floor(availableWidth));
  const columns = layoutColumns(width);
  const contentWidth = width - CANVAS_PADDING * 2;
  const clusterWidth = Math.floor((contentWidth - Math.max(0, columns - 1) * CLUSTER_GAP) / columns);
  const vectorNodes = sortedNodes(nodes.filter((node) => node.type === "vector"));
  const vectorIndexes = new Map(vectorNodes.map((node, index) => [node.id, index]));
  const capabilityVectorIndexes = new Map(
    nodes
      .filter((node) => node.type === "capability" && node.vector !== null && vectorIndexes.has(node.vector))
      .map((node) => [node.id, vectorIndexes.get(node.vector!)!]),
  );
  const coreNodes = orderedCoreNodes(nodes.filter((node) => node.type === "core"), edges, capabilityVectorIndexes);
  const coreIndexes = new Map(coreNodes.map((node, index) => [node.id, index]));
  const capabilitiesByVector = new Map<string | null, EssenceMapNode[]>();
  for (const node of nodes.filter((candidate) => candidate.type === "capability")) {
    const key = vectorIndexes.has(node.vector ?? "") ? node.vector : null;
    const group = capabilitiesByVector.get(key) ?? [];
    group.push(node);
    capabilitiesByVector.set(key, group);
  }
  const clusterKeys = vectorNodes.map((node) => node.id);
  if (capabilitiesByVector.has(null)) clusterKeys.push("__unassigned__");
  const layouts: EssenceMapNodeLayout[] = [];
  const clusters: EssenceMapClusterLayout[] = [];
  let y = CANVAS_PADDING;
  for (let rowStart = 0; rowStart < clusterKeys.length; rowStart += columns) {
    const rowKeys = clusterKeys.slice(rowStart, rowStart + columns);
    const rowClusters: EssenceMapClusterLayout[] = [];
    for (const [column, key] of rowKeys.entries()) {
      const vector = vectorNodes.find((node) => node.id === key) ?? null;
      const capabilities = orderedCapabilities(capabilitiesByVector.get(vector?.id ?? null) ?? [], edges, coreIndexes);
      const clusterHeight = CLUSTER_HEADER_HEIGHT
        + (vector ? NODE_HEIGHT + (capabilities.length > 0 ? NODE_GAP : 0) : 0)
        + gridHeight(capabilities.length, 1)
        + CLUSTER_PADDING;
      const x = CANVAS_PADDING + column * (clusterWidth + CLUSTER_GAP);
      const cluster = {
        vectorId: vector?.id ?? null,
        vector,
        colorIndex: rowStart + column,
        x,
        y,
        width: clusterWidth,
        height: clusterHeight,
        nodeIds: [] as string[],
      };
      const cardWidth = clusterWidth - CLUSTER_PADDING * 2;
      let nodeY = y + CLUSTER_HEADER_HEIGHT;
      if (vector) {
        placeNode(layouts, vector, x + CLUSTER_PADDING, nodeY, cardWidth);
        cluster.nodeIds.push(vector.id);
        nodeY += NODE_HEIGHT + NODE_GAP;
      }
      if (capabilities.length > 0) {
        for (const capability of capabilities) {
          placeNode(layouts, capability, x + CLUSTER_PADDING, nodeY, cardWidth);
          cluster.nodeIds.push(capability.id);
          nodeY += NODE_HEIGHT + NODE_GAP;
        }
      }
      rowClusters.push(cluster);
    }
    clusters.push(...rowClusters);
    y += Math.max(...rowClusters.map((cluster) => cluster.height)) + SECTION_GAP;
  }

  const clientNodes = sortedNodes(nodes.filter((node) => node.type === "client"));
  const regions: EssenceMapRegionLayout[] = [];
  if (coreNodes.length > 0) {
    const region = placeRegion(layouts, "core", coreNodes, y, contentWidth, columns);
    regions.push(region);
    y += region.height + SECTION_GAP;
  }
  if (clientNodes.length > 0) {
    const clientColumns = Math.min(2, columns);
    const region = placeRegion(layouts, "client", clientNodes, y, contentWidth, clientColumns);
    regions.push(region);
    y += region.height + SECTION_GAP;
  }
  const height = Math.max(150, y - SECTION_GAP + CANVAS_PADDING);
  return {
    width,
    height,
    nodes: layouts,
    nodeById: new Map(layouts.map((item) => [item.node.id, item])),
    clusters,
    regions,
  };
}

export function layoutEssenceMapWorld(
  projects: EssenceMapProject[],
  availableWidth: number,
): EssenceMapWorldLayout {
  const viewportWidth = Math.max(280, Math.floor(availableWidth));
  const mapWidth = Math.max(560, Math.min(900, viewportWidth - 48));
  const columns = viewportWidth >= 1100 ? 2 : 1;
  const gap = 72;
  const projectHeaderHeight = 58;
  const laidOut = projects.map((project) => ({ project, layout: layoutEssenceMapProject(project.nodes, mapWidth, project.edges) }));
  const rowHeights: number[] = [];
  for (let index = 0; index < laidOut.length; index += columns) {
    rowHeights.push(Math.max(...laidOut.slice(index, index + columns).map(({ layout }) => layout.height + projectHeaderHeight), 220));
  }
  const worldProjects: EssenceMapWorldProject[] = laidOut.map(({ project, layout }, index) => {
    const row = Math.floor(index / columns);
    const column = index % columns;
    const x = column * (mapWidth + gap);
    const y = rowHeights.slice(0, row).reduce((sum, height) => sum + height + gap, 0);
    return { ...project, layout, x, y };
  });
  const width = Math.max(mapWidth, columns * mapWidth + Math.max(0, columns - 1) * gap);
  const height = Math.max(220, rowHeights.reduce((sum, value) => sum + value, 0) + Math.max(0, rowHeights.length - 1) * gap);
  return {
    width,
    height,
    projects: worldProjects,
    projectBounds: new Map(worldProjects.map(({ file, x, y, layout }) => [file.project_path, { x, y, width: layout.width, height: layout.height + projectHeaderHeight }])),
  };
}
