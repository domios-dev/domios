import { getAgentDescriptor } from "./agentDescriptorCache";
import { getPluginProviderMeta } from "./pluginProviderMeta";
import type { Tab } from "./types";

export interface ResolvedTabProvider {
  id: string;
  displayName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  kind: "agent" | "shell";
}

export function resolveTabProvider(tab: Tab | null | undefined): ResolvedTabProvider | null {
  if (!tab) return null;

  if (tab.agent_type) {
    const d = getAgentDescriptor(tab.agent_type);
    if (d) {
      return {
        id: d.id,
        displayName: d.display_name,
        iconSvg: d.icon_svg,
        iconColor: d.icon_color,
        iconHint: d.icon_hint,
        kind: d.transport === "chat_backend" ? "shell" : "agent",
      };
    }
    return fallbackAgentProvider(tab.agent_type);
  }

  const backing = tab.backing;
  if (backing?.kind === "chat_backend") {
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

function fallbackAgentProvider(id: string): ResolvedTabProvider {
  const known: Record<string, { displayName: string; iconHint: string; iconColor?: string }> = {
    claude: { displayName: "Claude Code", iconHint: "C", iconColor: "#d97757" },
    codex: { displayName: "Codex", iconHint: "</>", iconColor: "#58a6ff" },
    cursor: { displayName: "Cursor", iconHint: "Cu", iconColor: "#d6d9e0" },
    opencode: { displayName: "OpenCode", iconHint: "OC", iconColor: "#f0b429" },
  };
  const meta = known[id] ?? {
    displayName: id,
    iconHint: id.slice(0, 2).toUpperCase(),
  };
  return {
    id,
    displayName: meta.displayName,
    iconColor: meta.iconColor,
    iconHint: meta.iconHint,
    kind: "agent",
  };
}
