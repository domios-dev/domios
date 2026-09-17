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
  status?: string;
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
  documents?: string[];
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
  if (node === null) return false;
  const allowed = new Set(["kind", "id", "type", "label", "vector", "source", "status", "runs_on_reason"]);
  if (Object.keys(node).some((key) => !allowed.has(key))) return false;
  const hasStatus = node.status === undefined || stringValue(node.status);
  return node !== null
    && node.kind === "node"
    && stringValue(node.id)
    && NODE_TYPES.includes(node.type as EssenceMapNodeType)
    && stringValue(node.label)
    && (node.vector === null || stringValue(node.vector))
    && sourceValue(node.source)
    && hasStatus
    && (node.type === "capability" ? node.status !== undefined : node.status === undefined)
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

function layoutEssenceMapProjectGrid(
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

function layoutEssenceMapProjectClustered(nodes: EssenceMapNode[], availableWidth: number, edges: EssenceMapEdge[] = []): EssenceMapProjectLayout {
  const width = Math.max(560, Math.floor(availableWidth));
  // Nodes are dots with labels beside them, not cards: a card-sized footprint tiles
  // instead of clustering, which is what made every earlier arrangement a wall of boxes.
  const cardWidth = 14;
  const nodeH = 14;
  const height = Math.max(720, Math.min(1180, 470 + nodes.length * 9));
  const center = { x: width / 2, y: height / 2 };
  const vectors = sortedNodes(nodes.filter((node) => node.type === "vector"));
  const capabilities = sortedNodes(nodes.filter((node) => node.type === "capability"));
  const core = sortedNodes(nodes.filter((node) => node.type === "core"));
  const clients = sortedNodes(nodes.filter((node) => node.type === "client"));
  const vectorRadius = Math.min(250, Math.min(width, height) / 2 - 90);
  const clientRadius = Math.min(330, Math.min(width, height) / 2 - 40);
  const positions = new Map<string, { x: number; y: number }>();
  const put = (node: EssenceMapNode, x: number, y: number) => positions.set(node.id, { x: Math.max(cardWidth / 2 + 4, Math.min(width - cardWidth / 2 - 4, x)), y: Math.max(48, Math.min(height - 48, y)) });
  // Seat strongly connected clusters next to each other on the ring, so their edges
  // stay short instead of crossing the middle. Seven hubs is 720 orders: test them all.
  const clusterOf = new Map<string, string>();
  vectors.forEach((vector) => clusterOf.set(vector.id, vector.id));
  capabilities.forEach((node) => { if (node.vector) clusterOf.set(node.id, node.vector); });
  const pairWeight = new Map<string, number>();
  for (const edge of edges) {
    const from = clusterOf.get(edge.from); const to = clusterOf.get(edge.to);
    if (!from || !to || from === to) continue;
    const key = from < to ? `${from}|${to}` : `${to}|${from}`;
    pairWeight.set(key, (pairWeight.get(key) ?? 0) + 1);
  }
  const ringCost = (candidate: EssenceMapNode[]): number => {
    const slot = new Map(candidate.map((vector, index) => [vector.id, index]));
    let cost = 0;
    for (const [key, weight] of pairWeight) {
      const [a, b] = key.split("|");
      const ia = slot.get(a); const ib = slot.get(b);
      if (ia === undefined || ib === undefined) continue;
      const gap = Math.abs(ia - ib);
      cost += weight * Math.min(gap, candidate.length - gap);
    }
    return cost;
  };
  const ringOrder = ((): EssenceMapNode[] => {
    if (vectors.length < 4 || vectors.length > 8 || pairWeight.size === 0) return vectors;
    const [head, ...rest] = vectors;
    let best = vectors; let bestCost = Number.POSITIVE_INFINITY;
    const walk = (remaining: EssenceMapNode[], prefix: EssenceMapNode[]) => {
      if (remaining.length === 0) {
        const candidate = [head, ...prefix];
        const cost = ringCost(candidate);
        if (cost < bestCost) { bestCost = cost; best = candidate; }
        return;
      }
      for (let i = 0; i < remaining.length; i += 1) walk([...remaining.slice(0, i), ...remaining.slice(i + 1)], [...prefix, remaining[i]]);
    };
    walk(rest, []);
    return best;
  })();
  ringOrder.forEach((node, index) => { const angle = -Math.PI / 2 + (index / Math.max(ringOrder.length, 1)) * Math.PI * 2; put(node, center.x + Math.cos(angle) * vectorRadius, center.y + Math.sin(angle) * vectorRadius); });
  const byVector = new Map<string, EssenceMapNode[]>();
  for (const node of capabilities) { const group = byVector.get(node.vector ?? "") ?? []; group.push(node); byVector.set(node.vector ?? "", group); }
  // Fan each hub's capabilities on a short arc pointing away from the centre: the
  // spread shrinks as the group grows so a large cluster stays a cluster.
  vectors.forEach((vector) => { const group = byVector.get(vector.id) ?? []; const anchor = positions.get(vector.id)!; const spread = Math.min(0.5, 2 / Math.max(group.length, 1)); const reach = 58 + group.length * 7; const outward = Math.atan2(anchor.y - center.y, anchor.x - center.x); group.forEach((node, index) => { const angle = outward + (index - (group.length - 1) / 2) * spread; put(node, anchor.x + Math.cos(angle) * reach, anchor.y + Math.sin(angle) * reach); }); });
  for (const node of capabilities) if (!positions.has(node.id)) put(node, center.x, center.y - 112);
  core.forEach((node, index) => { const angle = index * 2.4; const radius = Math.min(112, 50 + core.length * 30); put(node, center.x + Math.cos(angle) * radius, center.y + Math.sin(angle) * radius); });
  clients.forEach((node, index) => { const angle = -Math.PI / 2 + (index / Math.max(clients.length, 1)) * Math.PI * 2; put(node, center.x + Math.cos(angle) * clientRadius, center.y + Math.sin(angle) * clientRadius); });
  const order = [...vectors, ...capabilities, ...core, ...clients];
  const clampPoint = (point: { x: number; y: number }) => { point.x = Math.max(cardWidth / 2 + 4, Math.min(width - cardWidth / 2 - 4, point.x)); point.y = Math.max(48, Math.min(height - 48, point.y)); };
  // Separation reasons about the label, not the dot. A 14px dot with a 120px label
  // beside it clears a dot-sized gap while its text lies across its neighbour's,
  // which is what put a whole cluster's names on one unreadable line.
  const labelWidth = (node: EssenceMapNode): number => Math.min(150, node.label.length * 5.4 + 8);
  const boxOf = (node: EssenceMapNode, point: { x: number; y: number }) => {
    const half = cardWidth / 2;
    if (node.type === "capability") {
      const toLeft = point.x < width / 2;
      const label = labelWidth(node);
      return { x0: point.x - half - (toLeft ? label : 0), x1: point.x + half + (toLeft ? 0 : label), y0: point.y - nodeH / 2, y1: point.y + nodeH / 2 };
    }
    const box = node.type === "vector" ? 150 : 96;
    const lines = Math.max(1, Math.ceil((node.label.length * 5.6) / box));
    return { x0: point.x - box / 2, x1: point.x + box / 2, y0: point.y - nodeH / 2, y1: point.y + nodeH / 2 + 6 + lines * 13 };
  };
  // Fixed-iteration separation pass: deterministic, converges, leaves no loop running.
  for (let pass = 0; pass < 260; pass += 1) {
    let moved = false;
    for (let i = 0; i < order.length; i += 1) {
      for (let j = i + 1; j < order.length; j += 1) {
        const a = positions.get(order[i].id)!; const b = positions.get(order[j].id)!;
        const boxA = boxOf(order[i], a); const boxB = boxOf(order[j], b);
        const overlapX = Math.min(boxA.x1, boxB.x1) - Math.max(boxA.x0, boxB.x0);
        const overlapY = Math.min(boxA.y1, boxB.y1) - Math.max(boxA.y0, boxB.y0);
        if (overlapX <= 0 || overlapY <= 0) continue;
        moved = true;
        // Cap the shove so one huge label cannot fling a node across the map.
        if (overlapX < overlapY) { const push = (Math.min(overlapX, 24) / 2 + 0.5) * (b.x - a.x < 0 ? -1 : 1); a.x -= push; b.x += push; }
        else { const push = (Math.min(overlapY, 24) / 2 + 0.5) * (b.y - a.y < 0 ? -1 : 1); a.y -= push; b.y += push; }
        clampPoint(a); clampPoint(b);
      }
    }
    if (!moved) break;
  }
  const layouts = order.map((node) => { const point = positions.get(node.id) ?? center; return { node, x: point.x - cardWidth / 2, y: point.y - nodeH / 2, width: cardWidth, height: nodeH }; });
  const nodeById = new Map(layouts.map((item) => [item.node.id, item]));
  const boundsFor = (items: EssenceMapNodeLayout[]) => { const minX = Math.max(8, Math.min(...items.map((item) => item.x)) - 18); const minY = Math.max(8, Math.min(...items.map((item) => item.y)) - 28); const maxX = Math.min(width - 8, Math.max(...items.map((item) => item.x + item.width)) + 18); const maxY = Math.min(height - 8, Math.max(...items.map((item) => item.y + item.height)) + 28); return { x: minX, y: minY, width: maxX - minX, height: maxY - minY }; };
  const clusters: EssenceMapClusterLayout[] = vectors.map((vector, index) => { const items = layouts.filter((item) => item.node.id === vector.id || item.node.vector === vector.id); const bounds = boundsFor(items); return { vectorId: vector.id, vector, colorIndex: index, ...bounds, nodeIds: items.map((item) => item.node.id) }; });
  const unassigned = layouts.filter((item) => item.node.type === "capability" && !item.node.vector); if (unassigned.length) clusters.push({ vectorId: null, vector: null, colorIndex: vectors.length, ...boundsFor(unassigned), nodeIds: unassigned.map((item) => item.node.id) });
  const regions = ([{ type: "core" as const, label: "Core parts", items: layouts.filter((item) => item.node.type === "core") }, { type: "client" as const, label: "Clients", items: layouts.filter((item) => item.node.type === "client") }]).filter(({ items }) => items.length > 0).map(({ type, label, items }) => ({ type, label, ...boundsFor(items), nodeIds: items.map((item) => item.node.id) }));
  return { width, height, nodes: layouts, nodeById, clusters, regions };
}

export function layoutEssenceMapProject(nodes: EssenceMapNode[], availableWidth: number, edges: EssenceMapEdge[] = []): EssenceMapProjectLayout {
  return availableWidth < 560 ? layoutEssenceMapProjectGrid(nodes, availableWidth, edges) : layoutEssenceMapProjectClustered(nodes, availableWidth, edges);
}

export function layoutEssenceMapWorld(
  projects: EssenceMapProject[],
  availableWidth: number,
): EssenceMapWorldLayout {
  const viewportWidth = Math.max(280, Math.floor(availableWidth));
  const mapWidth = Math.max(760, Math.min(900, viewportWidth - 48));
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
