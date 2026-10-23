import { UpdateChannelNotice } from "../shared/UpdateChannelNotice";
import { ComponentStatus } from "../shared/ComponentStatus";
// plugin rows (installed + marketplace) with status/update/installed badges.
import { useOptionalUpdatePanel } from "../../hooks/useUpdatePanel";
import { useState } from "react";
import type { CatalogEntry, CatalogFilter } from "./pluginCatalog";
import { catalogKindLabel } from "./pluginCatalog";
import { PluginIcon } from "./PluginIcon";
import { iconOf } from "./shared";
import type { ChannelRecord } from "./types";
import { DropdownMenu } from "../shared/DropdownMenu";
import { ChevronRight, Trash2 } from "lucide-react";
import { KebabGlyph } from "../shared/RowMenuButton";
import type { PluginActivity } from "../../lib/types";
import { resolvePluginActivityDisplay, type PluginActivityDisplayMap } from "../../lib/usePluginActivity";


const CORE_CHANNEL = "codeterm-core";
const PLUGINS_CHANNEL = "codeterm-plugins";
const PROTECTED_CHANNELS = new Set([CORE_CHANNEL, PLUGINS_CHANNEL]);

const FILTERS: Array<{ key: CatalogFilter; label: string }> = [
  { key: "all", label: "All" },
  { key: "installed", label: "Installed" },
  { key: "marketplace", label: "Marketplace" },
  { key: "updates", label: "Updates" },
  { key: "channels", label: "Channels" },
];

export function PluginList({
  entries,
  selectedId,
  onSelect,
  query,
  onQuery,
  filter,
  onFilter,
  updateCount,
  onRemoveChannel,
  activities,
  activityDisplayMap,
  channels,
  selectedChannel,
  channelPluginCounts,
  onSelectChannel,
}: {
  entries: CatalogEntry[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  query: string;
  onQuery: (q: string) => void;
  filter: CatalogFilter;
  onFilter: (f: CatalogFilter) => void;
  updateCount: number;
  onRemoveChannel: (channel: string, installedCount: number) => void;
  activities: Map<string, PluginActivity>;
  activityDisplayMap: PluginActivityDisplayMap;
  channels: ChannelRecord[];
  selectedChannel: string | null;
  channelPluginCounts: Map<string, number>;
  onSelectChannel: (name: string) => void;
}) {
  const updatePanel = useOptionalUpdatePanel();
  const sections = groupBySource(entries);
  const channelRows = filterChannels(channels, query);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const toggleSection = (key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <input
        data-testid="extensions-search"
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        placeholder="Search plugins..."
        style={{
          fontSize: 12.5,
          borderRadius: 6,
          padding: "7px 10px",
          border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
          background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
          color: "var(--ct-fg)",
          marginBottom: 10,
        }}
      />
      <div style={{ display: "flex", gap: 4, marginBottom: 12, flexWrap: "nowrap", overflowX: "auto" }}>
        {FILTERS.map((f) => {
          const active = filter === f.key;
          const badge = f.key === "updates" && updateCount > 0 ? ` ${updateCount}` : "";
          return (
            <button
              key={f.key}
              data-testid="extensions-filter"
              data-extension-filter={f.key}
              onClick={() => onFilter(f.key)}
              style={{
                fontSize: 11.5,
                borderRadius: 6,
                padding: "4px 9px",
                cursor: "pointer",
                border: "1px solid transparent",
                background: active ? "var(--ct-accent, #6366f1)" : "transparent",
                color: active ? "#fff" : "var(--ct-muted)",
              }}
            >
              {f.label}
              {badge}
            </button>
          );
        })}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 4, overflowY: "auto", minHeight: 0 }} data-testid="extensions-list">
        {filter === "channels" ? (
          <ChannelRows
            channels={channelRows}
            selectedChannel={selectedChannel}
            pluginCounts={channelPluginCounts}
            onSelect={onSelectChannel}
          />
        ) : (
          <>
            {sections.map((section) => (
              <div key={section.key} data-testid="extensions-section" data-extension-section={section.key}>
                <div style={{ display: "flex", alignItems: "center", gap: 6, margin: "8px 4px 5px", color: "var(--ct-muted)", fontSize: 10.5, fontWeight: 650 }}>
                  <button
                    type="button"
                    data-testid="extensions-section-toggle"
                    data-extension-section={section.key}
                    onClick={() => toggleSection(section.key)}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      border: "none",
                      background: "transparent",
                      color: "inherit",
                      cursor: "pointer",
                      display: "flex",
                      alignItems: "center",
                      gap: 5,
                      padding: 0,
                      font: "inherit",
                      textAlign: "left",
                    }}
                  >
                    <ChevronRight
                      size={12}
                      style={{ transform: collapsed.has(section.key) ? "none" : "rotate(90deg)", transition: "transform 0.12s", flex: "none" }}
                    />
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{section.label}</span>
                    <span style={{ opacity: 0.65 }}>({section.entries.length})</span>
                  </button>
                  {section.channel && !PROTECTED_CHANNELS.has(section.channel) && (
                    <DropdownMenu
                      items={[
                        {
                          label: "Delete channel",
                          icon: <Trash2 size={12} />,
                          danger: true,
                          onClick: () => onRemoveChannel(section.channel ?? "", section.installedCount),
                        },
                      ]}
                      className="ml-auto"
                      trigger={
                        <button
                          type="button"
                          data-testid="extensions-channel-actions"
                          data-channel-name={section.channel}
                          title={`Channel ${section.channel} actions`}
                          style={{
                            width: 20,
                            height: 20,
                            border: "none",
                            borderRadius: 5,
                            background: "transparent",
                            color: "var(--ct-muted)",
                            cursor: "pointer",
                            display: "grid",
                            placeItems: "center",
                          }}
                        >
                          <KebabGlyph />
                        </button>
                      }
                    />
                  )}
                </div>
                {section.channel && updatePanel?.diagnostics.filter(item => item.channel === section.channel).map(item => <div key={item.channel} className="px-2 py-1"><UpdateChannelNotice channel={item.channel} message={item.message} /></div>)}
                {!collapsed.has(section.key) && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                    {section.entries.map((e) => (
                      <PluginRow
                        key={e.id}
                        entry={e}
                        selected={e.id === selectedId}
                        activity={activities.get(e.id)}
                        display={resolvePluginActivityDisplay(e.id, activityDisplayMap)}
                        onSelect={() => onSelect(e.id)}
                      />
                    ))}
                  </div>
                )}
              </div>
            ))}
            {entries.length === 0 && (
              <div style={{ color: "var(--ct-muted)", fontSize: 12.5, padding: "8px 4px" }} data-testid="extensions-list-empty">No plugins match.</div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function filterChannels(channels: ChannelRecord[], query: string): ChannelRecord[] {
  const q = query.trim().toLowerCase();
  return [...channels]
    .filter((channel) => {
      if (!q) return true;
      return `${channel.name} ${channel.url} ${channel.kind}`.toLowerCase().includes(q);
    })
    .sort((a, b) => channelRank(a).localeCompare(channelRank(b)));
}

function channelRank(channel: ChannelRecord): string {
  if (channel.name === CORE_CHANNEL) return "0";
  if (channel.name === PLUGINS_CHANNEL) return "1";
  if (channel.system) return `2:${channel.name}`;
  return `3:${channel.name}`;
}

function ChannelRows({
  channels,
  selectedChannel,
  pluginCounts,
  onSelect,
}: {
  channels: ChannelRecord[];
  selectedChannel: string | null;
  pluginCounts: Map<string, number>;
  onSelect: (name: string) => void;
}) {
  if (channels.length === 0) {
    return <div style={{ color: "var(--ct-muted)", fontSize: 12.5, padding: "8px 4px" }} data-testid="extensions-channel-list-empty">No channels match.</div>;
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4 }} data-testid="extensions-channel-list">
      {channels.map((channel) => {
        const selected = channel.name === selectedChannel;
        const count = pluginCounts.get(channel.name) ?? 0;
        return (
          <button
            key={channel.name}
            type="button"
            data-testid="extensions-channel-row"
            data-channel-name={channel.name}
            onClick={() => onSelect(channel.name)}
            style={{
              display: "grid",
              gridTemplateColumns: "1fr auto",
              alignItems: "center",
              gap: 8,
              width: "100%",
              minWidth: 0,
              border: "none",
              borderRadius: 8,
              padding: "8px 10px",
              background: selected ? "var(--ct-bg-selected)" : "transparent",
              color: "var(--ct-fg)",
              cursor: "pointer",
              textAlign: "left",
            }}
            onMouseEnter={(e) => {
              if (!selected) e.currentTarget.style.background = "var(--ct-bg-hover)";
            }}
            onMouseLeave={(e) => {
              if (!selected) e.currentTarget.style.background = "transparent";
            }}
          >
            <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
              <span style={{ fontSize: 12.5, fontWeight: 560, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{channel.name}</span>
              <ChannelBadge channel={channel} />
            </span>
            <span style={{ color: "var(--ct-muted)", fontSize: 11, whiteSpace: "nowrap" }}>
              {count} plugin{count === 1 ? "" : "s"}
            </span>
          </button>
        );
      })}
    </div>
  );
}

function groupBySource(entries: CatalogEntry[]): Array<{ key: string; label: string; channel?: string; installedCount: number; entries: CatalogEntry[] }> {
  const sections = new Map<string, { key: string; label: string; channel?: string; installedCount: number; entries: CatalogEntry[] }>();
  for (const entry of entries) {
    const key = entry.channel
      ? `channel:${entry.channel}`
      : entry.dev
        ? "workspace"
        : entry.system
          ? "bundled"
          : "installed";
    const label = entry.channel ? channelLabel(entry.channel) : entry.dev ? "Workspace" : entry.system ? "Bundled" : "Installed";
    const section = sections.get(key) ?? { key, label, channel: entry.channel ?? undefined, installedCount: 0, entries: [] };
    section.entries.push(entry);
    if (entry.installed) section.installedCount += 1;
    sections.set(key, section);
  }
  return [...sections.values()].sort((a, b) => sectionRank(a).localeCompare(sectionRank(b)));
}

function sectionRank(section: { key: string; label: string }): string {
  if (section.key === `channel:${CORE_CHANNEL}`) return "0";
  if (section.key === "bundled") return "1";
  if (section.key === "workspace") return "2";
  if (section.key === `channel:${PLUGINS_CHANNEL}`) return "3";
  if (section.key.startsWith("channel:")) return `4:${section.label}`;
  return "5";
}

function channelLabel(channel: string): string {
  if (channel === CORE_CHANNEL) return "Core";
  if (channel === PLUGINS_CHANNEL) return "Domios Plugins";
  return channel;
}

function PluginRow({
  entry,
  selected,
  onSelect,
  activity,
  display,
}: {
  entry: CatalogEntry;
  selected: boolean;
  activity?: PluginActivity;
  display: ReturnType<typeof resolvePluginActivityDisplay>;
  onSelect: () => void;
}) {
  const updates = useOptionalUpdatePanel();
  const updateAvailable = updates ? updates.pluginHasUpdate(entry.id) : entry.updateAvailable;
  const loadFailure = entry.loadError && !entry.loadError.keptPrevious ? entry.loadError : null;
  const channelFailed = entry.channel !== CORE_CHANNEL && Boolean(entry.channel && updates?.snapshot.errors[entry.channel]);
  const running = activity?.status === "running";
  const state = running ? "active" : loadFailure || channelFailed ? "failed" : updateAvailable ? "available" : entry.installed ? "installed" : "marketplace";
  const label = running ? activity.label : loadFailure ? `${entry.displayName} failed to load; select for details` : channelFailed ? `${entry.displayName} update check failed` : updateAvailable ? "Update available" : entry.installed ? `${entry.displayName} installed` : `${entry.displayName} available to install`;
  const description = entry.pitch || entry.description || catalogKindLabel(entry);
  const versionLabel = entry.version && entry.remoteVersion && entry.version !== entry.remoteVersion
    ? `${entry.version} → ${entry.remoteVersion}` : entry.version ?? entry.remoteVersion ?? "";
  return (
    <div>
      <div
        role="button"
        data-testid="extensions-plugin-row"
        data-plugin-id={entry.id}
        data-plugin-installed={entry.installed ? "true" : "false"}
        data-plugin-update-available={updateAvailable ? "true" : "false"}
        data-plugin-version={entry.version ?? ""}
        data-plugin-channel={entry.channel ?? ""}
        data-plugin-verified={entry.verified ? "true" : "false"}
        data-plugin-load-error={entry.loadError ? "true" : "false"}
        tabIndex={0}
        onClick={onSelect}
        onKeyDown={(e) => {
          if (e.code === "Enter" || e.code === "Space") { e.preventDefault(); onSelect(); }
        }}
        style={{
          display: "grid",
          gridTemplateColumns: "20px minmax(0,1fr) auto 14px",
          alignItems: "center",
          minWidth: 0,
          gap: 8,
          padding: "8px 10px",
          borderRadius: 8,
          cursor: "pointer",
          background: selected ? "var(--ct-bg-selected)" : "transparent",
        }}
        onMouseEnter={(e) => {
          if (!selected) e.currentTarget.style.background = "var(--ct-bg-hover)";
        }}
        onMouseLeave={(e) => {
          if (!selected) e.currentTarget.style.background = "transparent";
        }}
      >
        <span className="flex h-5 w-5 items-center justify-center"><PluginIcon plugin={iconOf(entry)} size={15} /></span>
        <span className="flex min-w-0 items-baseline gap-2" title={description}>
          <span className="max-w-[50%] shrink-0 truncate text-xs font-medium">{entry.displayName}</span>
          <span className="min-w-0 truncate text-[11px] text-[var(--ct-muted)]">{description}</span>
        </span>
        <span title={versionLabel} className="max-w-[110px] truncate text-[10px] tabular-nums text-[var(--ct-muted)]">{versionLabel}</span>
        <span data-testid={running ? "extensions-plugin-activity" : undefined} title={running ? `${display.displayName}: ${activity.label}` : label}><ComponentStatus state={state} label={label} /></span>
      </div>

    </div>
  );
}

function ChannelBadge({ channel }: { channel: ChannelRecord }) {
  return (
    <span
      data-testid="extensions-channel-badge"
      style={{
        fontSize: 9.5,
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        letterSpacing: 0,
        textTransform: "uppercase",
        borderRadius: 4,
        padding: "1px 5px",
        flex: "none",
        color: "var(--ct-muted)",
        border: "1px solid var(--ct-border-default)",
        background: channel.system ? "var(--ct-bg-active)" : "transparent",
      }}
    >
      {channel.name === CORE_CHANNEL || channel.system ? "Official" : "Custom"}
    </span>
  );
}
