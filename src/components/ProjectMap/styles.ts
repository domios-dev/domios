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
