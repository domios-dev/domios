export const KIND_COLORS: Record<string, string> = {
  file: "#c8b770",
  module: "#a99a60",
  class: "#6c8cc7",
  function: "#c9d1d9",
  method: "#8a8e96",
  variable: "#5a5e66",
  type: "#7fa9d6",
};

export const FALLBACK_KIND_COLOR = "#8a8e96";

export function colorForKind(kind: string): string {
  return KIND_COLORS[kind] ?? FALLBACK_KIND_COLOR;
}

export const EDGE_KIND_RGB: Record<string, string> = {
  contain: "120,128,140",
  invoke: "201,209,217",
  bridge: "240,200,80",
};

export function edgeRgbFor(kind: string): string {
  return EDGE_KIND_RGB[kind] ?? EDGE_KIND_RGB.invoke;
}

export const NODE_KINDS = ["file", "module", "class", "function", "method"] as const;

const CLUSTER_PALETTE = [
  "#6c8cc7",
  "#c8b770",
  "#7fa9d6",
  "#a08acc",
  "#7fb39a",
  "#cc8a7f",
  "#9ac786",
  "#b07fa9",
  "#d6a070",
  "#7fa6cc",
];

export function clusterKeyFromPath(path: string | null | undefined): string {
  if (!path) return "_root";
  const parts = path.split("/").filter(Boolean);
  if (parts.length === 0) return "_root";
  if (parts.length === 1) return parts[0];
  return `${parts[0]}/${parts[1]}`;
}

export function colorForCluster(key: string): string {
  let hash = 0;
  for (let i = 0; i < key.length; i++) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0;
  }
  return CLUSTER_PALETTE[hash % CLUSTER_PALETTE.length];
}

const MIN_SIZE = 3;
const MAX_SIZE = 12;
export function sizeFromImportance(
  importance: number | null | undefined,
  minImportance: number,
  maxImportance: number,
): number {
  if (importance == null || maxImportance <= minImportance) return MIN_SIZE + 1;
  const t = (importance - minImportance) / (maxImportance - minImportance);
  const eased = Math.sqrt(Math.max(0, Math.min(1, t)));
  return MIN_SIZE + eased * (MAX_SIZE - MIN_SIZE);
}
