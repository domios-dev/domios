// Pure, view-free logic for the titlebar extensions tray. Kept separate from
// the component so it can be unit-tested without the React/SVG import chain.

export interface TrayPluginData {
  id: string;
  display_name?: string | null;
  config_help?: string | null;
  icon_svg?: string | null;
  capabilities?: { glance_view?: boolean } | null;
}

export type ConfigPlugins = Record<string, { pinned?: boolean } | undefined>;

/** Event the tray dispatches and ExtensionsView listens for, to deep-link into a
 * plugin's detail (id === null clears the selection / opens the list root). */
export const SELECT_EXTENSION_EVENT = "codeterm:select-extension";

/** Pinned plugins, in snapshot order (the tray mirrors the list ordering). */
export function derivePinned(plugins: TrayPluginData[], cfgPlugins: ConfigPlugins): TrayPluginData[] {
  const pinnedIds = new Set(
    Object.entries(cfgPlugins)
      .filter(([, p]) => p?.pinned)
      .map(([id]) => id),
  );
  return plugins.filter((p) => pinnedIds.has(p.id));
}

/** Open the Extensions surface, deep-linking to `id` (or the root when null). */
export function selectExtension(id: string | null, onOpen: () => void): void {
  window.dispatchEvent(new CustomEvent(SELECT_EXTENSION_EVENT, { detail: { id } }));
  onOpen();
}
