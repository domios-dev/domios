export interface PluginProviderMeta {
  id: string;
  displayName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
}

/** Subset of a `plugin_manager_snapshot` entry the resolver cares about. */
interface SnapshotPluginEntry {
  id?: string | null;
  display_name?: string | null;
  icon_svg?: string | null;
  icon_color?: string | null;
}

export interface PluginSnapshotLike {
  plugins?: SnapshotPluginEntry[] | null;
}

export function pluginProviderMetaFromSnapshot(
  snapshot: PluginSnapshotLike | null | undefined,
): PluginProviderMeta[] {
  const out: PluginProviderMeta[] = [];
  for (const p of snapshot?.plugins ?? []) {
    if (!p?.id) continue;
    out.push({
      id: p.id,
      displayName: p.display_name || p.id,
      iconSvg: p.icon_svg ?? null,
      iconColor: p.icon_color ?? null,
    });
  }
  return out;
}

let cache: Record<string, PluginProviderMeta> = {};
let generation = 0;
const subscribers = new Set<() => void>();

export function setPluginProviderMeta(list: PluginProviderMeta[]): void {
  const next: Record<string, PluginProviderMeta> = {};
  for (const m of list) next[m.id] = m;
  cache = next;
  generation += 1;
  for (const fn of subscribers) fn();
}

export function getPluginProviderMeta(id: string | null | undefined): PluginProviderMeta | undefined {
  if (!id) return undefined;
  return cache[id];
}

export function subscribePluginProviderMeta(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}

export function pluginProviderMetaVersion(): number {
  return generation;
}
