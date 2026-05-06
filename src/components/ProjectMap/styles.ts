export const KIND_COLORS: Record<string, string> = {
  file: "#c8b770",
  module: "#a99a60",
  class: "#6c8cc7",
  function: "#c9d1d9",
  method: "#8a8e96",
  variable: "#5a5e66",
  type: "#7fa9d6",
};

const FALLBACK_KIND_COLOR = "#8a8e96";

export function colorForKind(kind: string): string {
  return KIND_COLORS[kind] ?? FALLBACK_KIND_COLOR;
}
