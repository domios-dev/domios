import type { ModelInfo } from "./types";

// Per-plugin identity sourced from `plugin_manager_snapshot` (CB-P1 exposes
// icon/display-name/models per plugin). Shell panes have no `agent_type`, so the
// pane resolver looks identity up here by `backing.plugin_id` instead.
//
// React-free store (mirrors agentDescriptorCache) so the resolver and its unit
// tests can read it without pulling in React. The hook lives in
// `usePaneProvider.ts`.
export interface PluginProviderMeta {
  id: string;
  displayName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  availableModels?: ModelInfo[];
  defaultModel?: string | null;
}

/** Subset of a `plugin_manager_snapshot` entry the resolver cares about. */
interface SnapshotPluginEntry {
  id?: string | null;
  display_name?: string | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
  available_models?: ModelInfo[] | null;
  default_model?: string | null;
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
      iconHint: p.icon_hint ?? null,
      availableModels: p.available_models ?? undefined,
      defaultModel: p.default_model ?? null,
    });
  }
  return out;
}

let cache: Record<string, PluginProviderMeta> = {};
const subscribers = new Set<() => void>();

export function setPluginProviderMeta(list: PluginProviderMeta[]): void {
  const next: Record<string, PluginProviderMeta> = {};
  for (const m of list) next[m.id] = m;
  cache = next;
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
