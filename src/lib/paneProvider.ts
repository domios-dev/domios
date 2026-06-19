import { getAgentDescriptor } from "./agentDescriptorCache";
import { getPluginProviderMeta } from "./pluginProviderMeta";
import type { Pane } from "./types";

// One identity for a pane regardless of how it's backed: agent panes resolve via
// `agent_type` (ProviderDescriptor cache), chatBackend shell panes via
// `backing.plugin_id` (plugin-snapshot cache). Pure + react-free so it's unit
// testable; the React subscription lives in `usePaneProvider.ts`.
export interface ResolvedPaneProvider {
  id: string;
  displayName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  kind: "agent" | "shell";
}

export function resolvePaneProvider(pane: Pane | null | undefined): ResolvedPaneProvider | null {
  if (!pane) return null;

  if (pane.agent_type) {
    const d = getAgentDescriptor(pane.agent_type);
    if (d) {
      return {
        id: d.id,
        displayName: d.display_name,
        iconSvg: d.icon_svg,
        iconColor: d.icon_color,
        iconHint: d.icon_hint,
        kind: "agent",
      };
    }
  }

  const backing = pane.backing;
  if (backing?.kind === "chat_backend") {
    // Identity (kind=shell) comes from the pane's own backing, not from cache
    // presence — a shell resolves as a shell even before the snapshot loads, so
    // chat-UI gating and the icon branch never flip back to "plain terminal".
    const meta = getPluginProviderMeta(backing.plugin_id);
    return {
      id: backing.plugin_id,
      displayName: meta?.displayName ?? backing.plugin_id,
      iconSvg: meta?.iconSvg,
      iconColor: meta?.iconColor,
      iconHint: meta?.iconHint,
      kind: "shell",
    };
  }

  return null;
}
