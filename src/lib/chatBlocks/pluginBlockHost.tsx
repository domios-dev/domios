import type { PluginBlockHost, PluginBlockState, PluginLifecycleKind } from "@codeterm/chat-ui";
import { buildCatalog, type CatalogEntry } from "../../components/Extensions/pluginCatalog";
import { pluginActionPayload } from "../../components/Extensions/pluginActions";
import { PluginIcon } from "../../components/Extensions/PluginIcon";
import { selectExtension } from "../../components/Titlebar/extensionsTrayLogic";
import type { PluginSnapshot } from "../../components/Extensions/types";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import { pluginBlockState } from "./pluginBlockState";

export interface PluginBlockStore {
  get(): PluginSnapshotStoreState;
  subscribe(listener: () => void): () => void;
  /** Read-only refresh of the shared snapshot; honours the store's freshness window. */
  ensure(): Promise<unknown>;
  /** The Extensions view's own lifecycle path. */
  runAction(action: Record<string, unknown>): Promise<PluginSnapshot>;
}

function openPanel(panel: string) {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel } }));
}

/** Plugin card services over the shared Extensions snapshot; reads never mutate and actions need a click. */
export function createPluginBlockHost(store: PluginBlockStore): PluginBlockHost {
  let cachedFor: PluginSnapshotStoreState | null = null;
  let catalog: CatalogEntry[] = [];
  const states = new Map<string, PluginBlockState>();

  const currentCatalog = () => {
    const state = store.get();
    if (state !== cachedFor) {
      cachedFor = state;
      catalog = buildCatalog(state.snapshot);
      states.clear();
    }
    return { state, catalog };
  };
  const refresh = () => {
    void store.ensure().catch(() => {});
  };

  return {
    binding: {
      subscribe(listener) {
        const unsubscribe = store.subscribe(listener);
        window.addEventListener("focus", refresh);
        return () => {
          unsubscribe();
          window.removeEventListener("focus", refresh);
        };
      },
      read(pluginId) {
        const { state, catalog: entries } = currentCatalog();
        let result = states.get(pluginId);
        if (!result) {
          result = pluginBlockState(state, entries, pluginId);
          states.set(pluginId, result);
        }
        return result;
      },
      refresh,
    },
    async run(pluginId: string, kind: PluginLifecycleKind) {
      const entry = currentCatalog().catalog.find((candidate) => candidate.id === pluginId);
      if (!entry) throw new Error("this plugin is no longer listed");
      await store.runAction(pluginActionPayload(kind, entry));
    },
    openView: (pluginId) => openPanel(`view:${pluginId}`),
    openInPlugins: (pluginId) => selectExtension(pluginId, () => openPanel("extensions")),
    renderIcon(pluginId, size) {
      const entry = currentCatalog().catalog.find((candidate) => candidate.id === pluginId);
      return entry ? <PluginIcon plugin={{ icon_svg: entry.iconSvg, icon_color: entry.iconColor }} size={size} /> : null;
    },
  };
}
