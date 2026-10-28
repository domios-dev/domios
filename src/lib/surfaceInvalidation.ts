import type { PluginViewSurface } from "./surfaceTab";

/** Old instances, another plugin and another surface never refresh this frame. */
export function surfaceInvalidationMatches(payload: unknown, plugin: string, surface: PluginViewSurface): boolean {
  if (!payload || typeof payload !== "object") return false;
  const value = payload as Record<string, unknown>;
  return value.plugin === plugin && value.surface === surface.surfaceId
    && value.instance === surface.instanceId && value.tab === surface.tabId;
}
