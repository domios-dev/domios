import type { ActionKind, ActionOptions } from "./PluginDetailPanel";
import type { CatalogEntry } from "./pluginCatalog";

/** The daemon action for a plugin lifecycle click; Extensions and chat plugin cards both send exactly this. */
export function pluginActionPayload(
  kind: ActionKind,
  entry: CatalogEntry,
  options: ActionOptions = {},
): Record<string, unknown> {
  switch (kind) {
    case "install":
      return { kind: "install", pluginId: entry.id, channel: entry.channel ?? undefined };
    case "update":
      return { kind: "update", pluginId: entry.id };
    case "remove":
      return { kind: "remove", pluginId: entry.id, removeData: options.removeData ?? false };
    case "settings":
      return {
        kind: "settings",
        pluginId: entry.id,
        allowHosts: options.allowHosts ?? [],
        disallowHosts: options.disallowHosts ?? [],
      };
  }
}
