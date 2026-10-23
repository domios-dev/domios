import type { PluginCapabilities } from "../components/Extensions/types";

export function supportsPluginBubble(plugin: { capabilities?: PluginCapabilities | null } | undefined): boolean {
  return plugin?.capabilities?.status_bubble === true;
}
