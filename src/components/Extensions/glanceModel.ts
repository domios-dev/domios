import type { GlanceView } from "./ViewNodeRenderer";

export function buildGlanceView(view: GlanceView | null, configHelp?: string): GlanceView {
  const resolved = view ?? { nodes: [] };
  if (!configHelp) return resolved;
  return {
    ...resolved,
    nodes: [
      ...resolved.nodes,
      { kind: "button", label: "Configure with AI", action: "configure-with-ai" },
    ],
  };
}
