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
import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
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
import { type AppConfigShape } from "./shared";
import type { ChannelRecord, PluginSnapshot } from "./types";
import type { AppConfig, PluginActivity } from "../../lib/types";
import type { PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";
import { Plus } from "lucide-react";

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
  const [selectedChannel, setSelectedChannel] = useState<string | null>(null);
  const [addChannelOpen, setAddChannelOpen] = useState(false);

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
    async (key: string, action: Record<string, unknown>): Promise<PluginSnapshot | null> => {
      setBusy(key);
      setError(null);
      try {
        const snap = await invoke<PluginSnapshot>("plugin_manager_action", { action });
        const normalized = normalizeSnapshot(snap);
        setSnapshot(normalized);
        return normalized;
      } catch (e) {
        setError(String(e));
        return null;
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

  const channelPluginCounts = useMemo(() => {
    const byChannel = new Map<string, Set<string>>();
    for (const entry of catalog) {
      if (!entry.channel) continue;
      const ids = byChannel.get(entry.channel) ?? new Set<string>();
      ids.add(entry.id);
      byChannel.set(entry.channel, ids);
    }
    return new Map([...byChannel.entries()].map(([channel, ids]) => [channel, ids.size]));
  }, [catalog]);

  const selectedChannelRecord = selectedChannel
    ? snapshot.channels.find((channel) => channel.name === selectedChannel) ?? null
    : null;

  useEffect(() => {
    if (filter !== "channels") return;
    if (selectedChannel && snapshot.channels.some((channel) => channel.name === selectedChannel)) return;
    setSelectedChannel(snapshot.channels[0]?.name ?? null);
  }, [filter, selectedChannel, snapshot.channels]);

  const addChannel = useCallback(
    async ({ name, url }: { name: string; url: string }): Promise<boolean> => {
      const trimmedName = name.trim();
      const trimmedUrl = url.trim();
      if (!trimmedName || !trimmedUrl) {
        setError("Channel name and URL are required.");
        return false;
      }
      if (!/^[A-Za-z0-9._-]+$/.test(trimmedName)) {
        setError("Channel name can use letters, numbers, dots, underscores, and hyphens.");
        return false;
      }
      if (!looksLikeChannelUrl(trimmedUrl)) {
        setError("Channel URL must be a URL, local path, or git SSH address.");
        return false;
      }
      const added = await runAction("add-channel", { kind: "add-channel", url: trimmedUrl, name: trimmedName });
      if (!added) return false;
      const refreshed = await runAction("refresh-channel", { kind: "refresh-channel", channel: trimmedName });
      setSelectedChannel(trimmedName);
      setAddChannelOpen(false);
      if (!refreshed) return false;
      return true;
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
                channels={snapshot.channels}
                selectedChannel={selectedChannel}
                channelPluginCounts={channelPluginCounts}
                onSelectChannel={setSelectedChannel}
              />
            </div>
          </div>

          {/* Right pane */}
          <div style={{ flex: 1, minWidth: 0, overflowY: "auto", paddingBottom: 24, borderLeft: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingLeft: 20 }} data-testid="extensions-detail-pane">
            {filter === "channels" ? (
              <ChannelDetailPanel
                channel={selectedChannelRecord}
                pluginCount={selectedChannelRecord ? channelPluginCounts.get(selectedChannelRecord.name) ?? 0 : 0}
                busy={busy}
                addOpen={addChannelOpen}
                onAddOpenChange={setAddChannelOpen}
                onAdd={addChannel}
              />
            ) : selected ? (
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

function looksLikeChannelUrl(value: string): boolean {
  if (/^(git@|ssh:\/\/|https?:\/\/|file:\/\/)/.test(value)) return true;
  if (/^(\.?\.?\/|~\/|[A-Za-z]:[\\/])/.test(value)) return true;
  try {
    const parsed = new URL(value);
    return parsed.protocol.length > 1;
  } catch {
    return false;
  }
}

function ChannelDetailPanel({
  channel,
  pluginCount,
  busy,
  addOpen,
  onAddOpenChange,
  onAdd,
}: {
  channel: ChannelRecord | null;
  pluginCount: number;
  busy: string | null;
  addOpen: boolean;
  onAddOpenChange: (open: boolean) => void;
  onAdd: (input: { name: string; url: string }) => Promise<boolean>;
}) {
  return (
    <div style={{ minHeight: "100%", display: "flex", flexDirection: "column", gap: 18 }} data-testid="extensions-channel-detail">
      {channel ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12, minWidth: 0 }}>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontSize: 21, fontWeight: 650, letterSpacing: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{channel.name}</div>
              <div style={{ color: "var(--ct-muted)", fontSize: 12.5, marginTop: 3 }}>{channel.system ? "Official channel" : "Custom channel"}</div>
            </div>
            <span style={{
              flex: "none",
              fontSize: 10,
              textTransform: "uppercase",
              borderRadius: 4,
              padding: "2px 6px",
              color: "var(--ct-muted)",
              border: "1px solid var(--ct-border-default)",
              background: channel.system ? "var(--ct-bg-active)" : "transparent",
            }}>
              {channel.system ? "Official" : "Custom"}
            </span>
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 10 }} data-testid="extensions-channel-attributes">
            <Attribute label="URL" value={channel.url} wide />
            <Attribute label="Type" value={channel.kind} />
            <Attribute label="Trust" value={channel.verified || channel.trusted || channel.system ? "verified" : "unverified"} />
            <Attribute label="Plugins" value={String(pluginCount)} />
            <Attribute label="Source" value={channel.system ? "official" : "custom"} />
            <Attribute label="Version" value={channel.version ?? null} />
            <Attribute label="Last refresh" value={formatChannelDate(channel.last_refresh ?? channel.last_refreshed ?? channel.refreshed_at ?? null)} />
            <Attribute label="Cache path" value={channel.path ?? null} wide />
          </div>
        </div>
      ) : (
        <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--ct-muted)", fontSize: 13 }} data-testid="extensions-channel-empty">
          No channel selected.
        </div>
      )}
      <div style={{ marginTop: "auto", borderTop: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingTop: 14 }}>
        <AddChannelInline busy={busy} open={addOpen} onOpenChange={onAddOpenChange} onAdd={onAdd} />
      </div>
    </div>
  );
}

function Attribute({ label, value, wide = false }: { label: string; value: string | number | null | undefined; wide?: boolean }) {
  if (value === null || value === undefined || value === "") return null;
  return (
    <div style={{ minWidth: 0, gridColumn: wide ? "1 / -1" : undefined }}>
      <div style={{ color: "var(--ct-muted)", fontSize: 10.5, textTransform: "uppercase", marginBottom: 4 }}>{label}</div>
      <div style={{
        color: "var(--ct-fg)",
        fontSize: 12.5,
        border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))",
        background: "var(--ct-bg-elev, rgba(255,255,255,0.025))",
        borderRadius: 7,
        padding: "7px 8px",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      }} title={String(value)}>
        {value}
      </div>
    </div>
  );
}

function formatChannelDate(value: string | number | null | undefined): string | null {
  if (value === null || value === undefined || value === "") return null;
  const date = typeof value === "number" ? new Date(value) : new Date(value);
  if (Number.isNaN(date.getTime())) return String(value);
  return date.toLocaleString();
}

function AddChannelInline({
  busy,
  open,
  onOpenChange,
  onAdd,
}: {
  busy: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAdd: (input: { name: string; url: string }) => Promise<boolean>;
}) {
  if (!open) {
    return (
      <button type="button" data-testid="extensions-channel-add-open" onClick={() => onOpenChange(true)} style={addButtonStyle}>
        <Plus size={14} strokeWidth={1.8} />
        Add channel
      </button>
    );
  }
  return (
    <form
      style={{ display: "flex", flexDirection: "column", gap: 8 }}
      data-testid="extensions-channel-add-form"
      onSubmit={async (event) => {
        event.preventDefault();
        const form = event.currentTarget;
        const data = new FormData(form);
        const ok = await onAdd({
          name: String(data.get("name") ?? ""),
          url: String(data.get("url") ?? ""),
        });
        if (!ok) return;
        form.reset();
      }}
    >
      <input data-testid="extensions-channel-name-input" name="name" required placeholder="channel name" style={inputStyle} />
      <input data-testid="extensions-channel-url-input" name="url" required placeholder="channel URL" style={inputStyle} />
      <div style={{ display: "flex", gap: 8 }}>
        <button
          type="submit"
          data-testid="extensions-channel-add-submit"
          disabled={busy !== null}
          style={{ ...smallButtonStyle, flex: 1 }}
        >
          Add
        </button>
        <button type="button" data-testid="extensions-channel-add-cancel" onClick={() => onOpenChange(false)} style={{ ...smallButtonStyle, flex: 1 }}>
          Cancel
        </button>
      </div>
    </form>
  );
}

const addButtonStyle: CSSProperties = {
  width: "100%",
  minHeight: 34,
  borderRadius: 7,
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 7,
  fontSize: 12,
};

const smallButtonStyle: CSSProperties = {
  fontSize: 11.5,
  borderRadius: 6,
  padding: "6px 10px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
};

const inputStyle: CSSProperties = {
  fontSize: 12,
  borderRadius: 6,
  padding: "7px 9px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
  color: "var(--ct-fg)",
};

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
