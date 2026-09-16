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

export interface EssenceMapProjectLayout {
  width: number;
  height: number;
  nodes: EssenceMapNodeLayout[];
  nodeById: Map<string, EssenceMapNodeLayout>;
}

const NODE_HEIGHT = 76;
const NODE_GAP = 10;
const ROW_GAP = 20;
const CLUSTER_PADDING = 18;

export function layoutEssenceMapProject(
  nodes: EssenceMapNode[],
  availableWidth: number,
): EssenceMapProjectLayout {
  const width = Math.max(280, Math.floor(availableWidth));
  const columns = width >= 720 ? 4 : width >= 500 ? 3 : 2;
  const cardWidth = Math.max(124, Math.floor((width - (columns - 1) * NODE_GAP - CLUSTER_PADDING * 2) / columns));
  const byType = new Map<EssenceMapNodeType, EssenceMapNode[]>();
  for (const type of NODE_TYPES) byType.set(type, []);
  for (const node of nodes) byType.get(node.type)?.push(node);
  for (const group of byType.values()) group.sort((a, b) => a.id.localeCompare(b.id));

  const layouts: EssenceMapNodeLayout[] = [];
  let y = CLUSTER_PADDING;
  for (const type of NODE_TYPES) {
    const group = byType.get(type) ?? [];
    if (group.length === 0) continue;
    for (const [index, node] of group.entries()) {
      layouts.push({
        node,
        x: CLUSTER_PADDING + (index % columns) * (cardWidth + NODE_GAP),
        y: y + Math.floor(index / columns) * (NODE_HEIGHT + NODE_GAP),
        width: cardWidth,
        height: NODE_HEIGHT,
      });
    }
    y += Math.ceil(group.length / columns) * (NODE_HEIGHT + NODE_GAP) + ROW_GAP;
  }
  const height = Math.max(150, y - ROW_GAP + CLUSTER_PADDING);
  return { width, height, nodes: layouts, nodeById: new Map(layouts.map((item) => [item.node.id, item])) };
}
