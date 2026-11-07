import { getAgentDescriptor } from "./agentDescriptorCache";
import { getPluginProviderMeta } from "./pluginProviderMeta";
import type { Tab } from "./types";

export interface ResolvedTabProvider {
  id: string;
  displayName: string;
  iconSvg?: string | null;
  fallbackIconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  kind: "agent" | "shell";
}

export function resolveTabProvider(tab: Tab | null | undefined): ResolvedTabProvider | null {
  if (!tab) return null;

  if (tab.agent_type) {
    const d = getAgentDescriptor(tab.agent_type);
    if (d) {
      const profileId = tab.agent_runtime_config?.launchProfile;
      const profile = typeof profileId === "string" ? d.agent_runtime?.launchProfiles?.[profileId] : undefined;
      const genericIcon = d.agent_runtime?.launchProfiles !== undefined || typeof profileId === "string" ? undefined : d.icon_svg;
      return {
        id: d.id,
        displayName: runtimeAgentLabel(tab) ?? d.display_name,
        iconSvg: resolveAgentIcon(profile?.iconSvg, tab.agent_runtime_state?.iconSvg, genericIcon),
        fallbackIconSvg: genericIcon,
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

export function resolveAgentIcon(profile: string | null | undefined, registry: string | null | undefined, generic: string | null | undefined): string | null | undefined {
  return profile || registry || generic;
}

export function runtimeAgentLabel(tab: Tab): string | null {
  const state = tab.agent_runtime_state;
  return state?.agentName ? [state.agentName, state.binaryVersion].filter(Boolean).join(" · ") : null;
}
