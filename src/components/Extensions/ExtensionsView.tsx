/**
 * Extensions — a VS Code-like plugin manager. Left column: search, filters,
 * plugin list (installed inventory + channel marketplace, with installed/update
 * badges). Right pane: the selected plugin's detail with install/update/uninstall
 * actions, capabilities, settings, and a safe markdown description.
 *
 * The catalog merge/filter logic lives in `pluginCatalog.ts` (pure, unit-tested);
 * this component owns state + side effects (snapshot refresh, config, actions).
 * Backend: `plugin_manager_snapshot` / `plugin_manager_action` (web-mirrored via
 * bridge HTTP_MAP). Pinning is stored in `config.plugins.<id>.pinned`.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import {
  buildCatalog,
  filterCatalog,
  updateCount,
  type CatalogEntry,
  type CatalogFilter,
} from "./pluginCatalog";
import { PluginList } from "./PluginList";
import { PluginDetailPanel, type ActionKind, type ActionOptions } from "./PluginDetailPanel";
import { ChannelManager } from "./ChannelManager";
import { type AppConfigShape } from "./shared";
import type { PluginSnapshot } from "./types";
import type { AppConfig, PluginActivity } from "../../lib/types";
import type { PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";

const EMPTY_SNAPSHOT: PluginSnapshot = { plugins: [], channels: [], diff: [], marketplace: [] };

function normalizeSnapshot(snap: PluginSnapshot | null | undefined): PluginSnapshot {
  return {
    plugins: snap?.plugins ?? [],
    channels: snap?.channels ?? [],
    diff: snap?.diff ?? [],
    marketplace: snap?.marketplace ?? [],
  };
}

function actionPayload(
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
  }
}

export function ExtensionsView({
  visible,
  pluginActivities,
  pluginActivityDisplayMap,
}: {
  visible: boolean;
  pluginActivities: Map<string, PluginActivity>;
  pluginActivityDisplayMap: PluginActivityDisplayMap;
}) {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>(EMPTY_SNAPSHOT);
  const [config, setConfig] = useState<AppConfigShape | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<CatalogFilter>("all");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [channelsOpen, setChannelsOpen] = useState(false);

  const refresh = useCallback(async (refreshChannels = false) => {
    if (refreshChannels) setBusy("refresh-channel");
    try {
      const [snap, cfg] = await Promise.all([
        refreshChannels
          ? invoke<PluginSnapshot>("plugin_manager_action", { action: { kind: "refresh-channel" } })
          : invoke<PluginSnapshot>("plugin_manager_snapshot"),
        invoke<AppConfigShape>("get_config"),
      ]);
      setSnapshot(normalizeSnapshot(snap));
      setConfig(cfg ?? {});
      setError(null);
    } catch (e) {
      setError(refreshChannels ? `Couldn't refresh channels: ${String(e)}` : String(e));
      if (refreshChannels) {
        try {
          const snap = await invoke<PluginSnapshot>("plugin_manager_snapshot");
          setSnapshot(normalizeSnapshot(snap));
        } catch {
          // Keep the channel-refresh error visible.
        }
      }
    } finally {
      if (refreshChannels) setBusy(null);
    }
  }, []);

  useEffect(() => {
    if (visible) void refresh(true);
  }, [visible, refresh]);

  // Deep-link: the titlebar tray dispatches this to jump straight to a plugin.
  useEffect(() => {
    const onSelect = (e: Event) => {
      const id = (e as CustomEvent<{ id: string | null }>).detail?.id ?? null;
      setSelectedId(id);
    };
    window.addEventListener(SELECT_EXTENSION_EVENT, onSelect);
    return () => window.removeEventListener(SELECT_EXTENSION_EVENT, onSelect);
  }, []);

  const catalog = useMemo(() => buildCatalog(snapshot), [snapshot]);
  const visibleEntries = useMemo(() => filterCatalog(catalog, query, filter), [catalog, query, filter]);
  const updates = useMemo(() => updateCount(catalog), [catalog]);
  const selected = selectedId ? catalog.find((e) => e.id === selectedId) ?? null : null;

  const pinned = useMemo(() => {
    const s = new Set<string>();
    for (const [id, p] of Object.entries(config?.plugins ?? {})) if (p?.pinned) s.add(id);
    return s;
  }, [config]);

  const channelInstalledCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const entry of catalog) {
      if (entry.installed && entry.channel) counts.set(entry.channel, (counts.get(entry.channel) ?? 0) + 1);
    }
    return counts;
  }, [catalog]);

  const saveSetting = useCallback(async (pluginId: string, key: string, value: unknown) => {
    try {
      const cfg = await invoke<AppConfigShape>("get_config");
      const plugins = { ...(cfg.plugins ?? {}) };
      plugins[pluginId] = { ...(plugins[pluginId] ?? {}), [key]: value };
      await invoke("update_config", { config: { ...cfg, plugins } });
      setConfig({ ...cfg, plugins });
      setError(null);
    } catch (e) {
      setError(`Couldn't save ${pluginId} setting: ${String(e)}`);
    }
  }, []);

  const togglePin = useCallback(async (id: string) => {
    try {
      const cfg = await invoke<AppConfigShape>("get_config");
      const plugins = { ...(cfg.plugins ?? {}) };
      const entry = { ...(plugins[id] ?? {}) };
      entry.pinned = !entry.pinned;
      plugins[id] = entry;
      await invoke("update_config", { config: { ...cfg, plugins } });
      setConfig({ ...cfg, plugins });
      setError(null);
    } catch (e) {
      setError(`Couldn't pin ${id}: ${String(e)}`);
    }
  }, []);

  const runAction = useCallback(
    async (key: string, action: Record<string, unknown>): Promise<boolean> => {
      setBusy(key);
      setError(null);
      try {
        const snap = await invoke<PluginSnapshot>("plugin_manager_action", { action });
        setSnapshot(normalizeSnapshot(snap));
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const onPluginAction = useCallback(
    (kind: ActionKind, entry: CatalogEntry, options?: ActionOptions) => {
      void runAction(`${kind}:${entry.id}`, actionPayload(kind, entry, options));
    },
    [runAction],
  );

  const onRemoveChannel = useCallback(
    (channel: string, installedCount: number) => {
      const pluginText = installedCount === 1 ? "1 installed plugin" : `${installedCount} installed plugins`;
      if (!window.confirm(`Delete channel "${channel}" and remove ${pluginText} from it?`)) return;
      void runAction(`remove-channel:${channel}`, { kind: "remove-channel", channel, force: true });
    },
    [runAction],
  );

  return (
    <div className="h-full overflow-hidden" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }} data-testid="extensions-view">
      <div style={{ display: "flex", flexDirection: "column", height: "100%", maxWidth: 1100, margin: "0 auto", padding: "24px 24px 0" }}>
        {error && (
          <div
            role="alert"
            data-testid="extensions-error"
            style={{ fontSize: 12.5, color: "var(--ct-err, #e57373)", border: "1px solid var(--ct-err, rgba(229,115,115,0.4))", borderRadius: 8, padding: "9px 12px", marginBottom: 12, flex: "none" }}
          >
            {error}
          </div>
        )}

        <div style={{ display: "flex", gap: 20, flex: 1, minHeight: 0 }}>
          {/* Left column */}
          <div style={{ width: 320, flex: "none", display: "flex", flexDirection: "column", minHeight: 0 }} data-testid="extensions-sidebar">
            <div style={{ flex: 1, minHeight: 0 }}>
              <PluginList
                entries={visibleEntries}
                selectedId={selectedId}
                onSelect={setSelectedId}
                query={query}
                onQuery={setQuery}
                filter={filter}
                onFilter={setFilter}
                updateCount={updates}
                pinned={pinned}
                onTogglePin={togglePin}
                onRemoveChannel={onRemoveChannel}
                activities={pluginActivities}
                activityDisplayMap={pluginActivityDisplayMap}
              />
            </div>
            <ChannelManager
              busy={busy}
              onAction={runAction}
              channels={snapshot.channels}
              installedCounts={channelInstalledCounts}
              onRemoveChannel={onRemoveChannel}
              open={channelsOpen}
              onOpenChange={setChannelsOpen}
            />
          </div>

          {/* Right pane */}
          <div style={{ flex: 1, minWidth: 0, overflowY: "auto", paddingBottom: 24, borderLeft: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingLeft: 20 }} data-testid="extensions-detail-pane">
            {selected ? (
              <PluginDetailPanel
                entry={selected}
                config={config}
                busy={busy}
                activity={pluginActivities.get(selected.id)}
                activityDisplayMap={pluginActivityDisplayMap}
                onAction={onPluginAction}
                onSave={saveSetting}
                onUpdateFullConfig={async (next: AppConfig) => {
                  try {
                    await invoke("update_config", { config: next });
                    setConfig(next as unknown as AppConfigShape);
                    setError(null);
                  } catch (e) {
                    setError(`Couldn't save settings: ${String(e)}`);
                  }
                }}
                onGone={() => {
                  setSelectedId(null);
                  void refresh();
                }}
              />
            ) : (
              <EmptyState count={catalog.length} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function EmptyState({ count }: { count: number }) {
  return (
    <div style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", color: "var(--ct-muted)", textAlign: "center", padding: 24 }} data-testid="extensions-empty-state">
      <div style={{ fontSize: 13.5, marginBottom: 4 }}>Select a plugin</div>
      <div style={{ fontSize: 12 }}>
        {count} plugin{count === 1 ? "" : "s"} available - pick one to see its details and actions.
      </div>
    </div>
  );
}
