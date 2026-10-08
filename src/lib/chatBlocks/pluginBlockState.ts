import type { PluginBlockInfo, PluginBlockState } from "@codeterm/chat-ui";
import type { CatalogEntry } from "../../components/Extensions/pluginCatalog";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";

const PITCH_LIMIT = 160;

/** The card's one-line outcome: the manifest pitch, else the description's first sentence. */
export function pluginPitch(entry: Pick<CatalogEntry, "pitch" | "description">): string {
  const text = (entry.pitch ?? entry.description ?? "").replace(/\s+/g, " ").trim();
  if (entry.pitch) return text;
  const sentence = /^.*?[.!?](?=\s|$)/.exec(text)?.[0] ?? text;
  return sentence.length > PITCH_LIMIT ? `${sentence.slice(0, PITCH_LIMIT - 1).trimEnd()}…` : sentence;
}

export function pluginBlockInfo(entry: CatalogEntry): PluginBlockInfo {
  return {
    id: entry.id,
    name: entry.displayName,
    pitch: pluginPitch(entry),
    installed: entry.installed,
    updateAvailable: entry.updateAvailable,
    compatible: entry.compatible,
    blockedReason: entry.blockedReason,
    managed: entry.system || entry.dev,
    hasView: entry.capabilities?.view === true,
    pendingReload: entry.pendingReload === true,
    version: entry.version,
    remoteVersion: entry.remoteVersion,
    iconColor: entry.iconColor,
  };
}

type StoreView = Pick<PluginSnapshotStoreState, "hasLoaded" | "error">;

/** Honest card state from the shared Extensions snapshot: never a guess while the list is unknown. */
export function pluginBlockState(store: StoreView, catalog: readonly CatalogEntry[], pluginId: string): PluginBlockState {
  const entry = catalog.find((candidate) => candidate.id === pluginId);
  if (entry) return { kind: "ready", plugin: pluginBlockInfo(entry) };
  if (!store.hasLoaded) return store.error ? { kind: "unavailable", message: store.error } : { kind: "loading" };
  return { kind: "missing" };
}
