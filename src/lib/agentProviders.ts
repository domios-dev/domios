/**
 * Frontend provider adapter — backed entirely by the plugin registry.
 *
 * This file used to host a parallel hardcoded registry that each
 * plugin populated by calling `registerAgentProvider({...})`. That
 * was duplication: every plugin already registers itself via
 * `src/lib/plugins/registry.ts`, so this adapter just translates the
 * `ProviderPlugin` shape into the legacy `AgentProviderConfig` shape
 * that Sidebar / TabItem / ChatView / WorkspaceItem still consume.
 *
 * New code should import from `src/lib/plugins/registry.ts` directly
 * and treat `ProviderPlugin` as the source of truth. Once the last
 * consumer migrates we can delete this file.
 */
import { getAllPlugins, getPlugin } from "./plugins/registry";
import type { ImageSendStrategy } from "./plugins/types";

export type { ImageSendStrategy };

/** Legacy view of a provider — kept stable so existing components
 *  don't need refactoring in this pass. Each field is derived from
 *  the plugin's own metadata. */
export interface AgentProviderConfig {
  id: string;
  displayName: string;
  titlePattern: RegExp;
  imageSend?: ImageSendStrategy;
  noisePatterns?: RegExp[];
}

function toLegacyConfig(id: string): AgentProviderConfig | undefined {
  const p = getPlugin(id);
  if (!p) return undefined;
  return {
    id: p.meta.id,
    displayName: p.meta.displayName,
    // Fallback regex matches nothing — plugins without a titlePattern (rare) rely on PTY-byte detection only, which the host runs through `detectProvider()` in the plugin registry.
    titlePattern: p.titlePattern ?? /^_____never_match_____$/,
    imageSend: p.imageSend,
    noisePatterns: p.noisePatterns,
  };
}

export function getAgentProvider(id: string | null | undefined): AgentProviderConfig | undefined {
  if (!id) return undefined;
  return toLegacyConfig(id);
}

export function getAllProviders(): AgentProviderConfig[] {
  return getAllPlugins()
    .map(p => toLegacyConfig(p.meta.id))
    .filter((c): c is AgentProviderConfig => c !== undefined);
}

export function detectAgentFromTitle(title: string): string | undefined {
  for (const p of getAllPlugins()) {
    if (p.titlePattern?.test(title)) return p.meta.id;
  }
  return undefined;
}

/** Re-export from the plugin registry so existing callers keep working. */
export { isProviderNoise } from "./plugins/registry";
