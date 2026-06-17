// Left column of the Extensions surface: search box, filter tabs, and the
// plugin rows (installed + marketplace) with status/update/installed badges.
import { useState } from "react";
import type { CatalogEntry, CatalogFilter } from "./pluginCatalog";
import { catalogKindLabel } from "./pluginCatalog";
import { PluginIcon } from "./PluginIcon";
import { iconOf } from "./shared";
import { PinIcon } from "../Icons";
import { DropdownMenu } from "../shared/DropdownMenu";
import { ChevronRight, Ellipsis, Trash2 } from "lucide-react";

const CORE_CHANNEL = "codeterm-core";
const PLUGINS_CHANNEL = "codeterm-plugins";
const PROTECTED_CHANNELS = new Set([CORE_CHANNEL, PLUGINS_CHANNEL]);

const FILTERS: Array<{ key: CatalogFilter; label: string }> = [
  { key: "all", label: "All" },
  { key: "installed", label: "Installed" },
  { key: "marketplace", label: "Marketplace" },
  { key: "updates", label: "Updates" },
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
  pinned,
  onTogglePin,
  onRemoveChannel,
}: {
  entries: CatalogEntry[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  query: string;
  onQuery: (q: string) => void;
  filter: CatalogFilter;
  onFilter: (f: CatalogFilter) => void;
  updateCount: number;
  pinned: Set<string>;
  onTogglePin: (id: string) => void;
  onRemoveChannel: (channel: string, installedCount: number) => void;
}) {
  const sections = groupBySource(entries);
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
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        placeholder="Search extensions…"
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
      <div style={{ display: "flex", gap: 4, marginBottom: 12, flexWrap: "wrap" }}>
        {FILTERS.map((f) => {
          const active = filter === f.key;
          const badge = f.key === "updates" && updateCount > 0 ? ` ${updateCount}` : "";
          return (
            <button
              key={f.key}
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

      <div style={{ display: "flex", flexDirection: "column", gap: 4, overflowY: "auto", minHeight: 0 }}>
        {sections.map((section) => (
          <div key={section.key}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, margin: "8px 4px 5px", color: "var(--ct-muted)", fontSize: 10.5, fontWeight: 650 }}>
              <button
                type="button"
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
                      <Ellipsis size={14} />
                    </button>
                  }
                />
              )}
            </div>
            {!collapsed.has(section.key) && (
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {section.entries.map((e) => (
                  <PluginRow
                    key={e.id}
                    entry={e}
                    selected={e.id === selectedId}
                    pinned={pinned.has(e.id)}
                    onSelect={() => onSelect(e.id)}
                    onTogglePin={() => onTogglePin(e.id)}
                  />
                ))}
              </div>
            )}
          </div>
        ))}
        {entries.length === 0 && (
          <div style={{ color: "var(--ct-muted)", fontSize: 12.5, padding: "8px 4px" }}>No extensions match.</div>
        )}
      </div>
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
  if (channel === PLUGINS_CHANNEL) return "CodeTerm Plugins";
  return channel;
}

function PluginRow({
  entry,
  selected,
  pinned,
  onSelect,
  onTogglePin,
}: {
  entry: CatalogEntry;
  selected: boolean;
  pinned: boolean;
  onSelect: () => void;
  onTogglePin: () => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onSelect();
      }}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
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
      <div style={{ width: 26, height: 26, borderRadius: 6, display: "flex", alignItems: "center", justifyContent: "center", flex: "none", background: "var(--ct-bg-elev)", border: "1px solid var(--ct-border-subtle)" }}>
        <PluginIcon plugin={iconOf(entry)} size={15} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 12.5, fontWeight: 560, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
            {entry.displayName}
          </span>
          {entry.dev ? (
            <Badge tone="dev">Dev</Badge>
          ) : (
            entry.installed && <Badge tone="installed">Installed</Badge>
          )}
          {entry.updateAvailable && <Badge tone="update">Update</Badge>}
        </div>
        <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10, color: "var(--ct-muted)", marginTop: 2, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {catalogKindLabel(entry)}
          {entry.version ? ` · v${entry.version}` : entry.remoteVersion ? ` · v${entry.remoteVersion}` : ""}
        </div>
      </div>
      {entry.installed && (
        <button
          onClick={(ev) => {
            ev.stopPropagation();
            onTogglePin();
          }}
          title={pinned ? "Unpin from titlebar" : "Pin to titlebar"}
          style={{ background: "transparent", border: "none", cursor: "pointer", display: "flex", alignItems: "center", color: pinned ? "var(--ct-accent, #6366f1)" : "var(--ct-muted)", padding: 3 }}
        >
          <PinIcon size={13} filled={pinned} />
        </button>
      )}
    </div>
  );
}

function Badge({ tone, children }: { tone: "installed" | "update" | "dev"; children: React.ReactNode }) {
  const style =
    tone === "update"
      ? { background: "transparent", color: "var(--ct-fg)", border: "1px solid var(--ct-border-default)" }
      : tone === "dev"
        ? { background: "transparent", color: "var(--ct-fg)", border: "1px solid var(--ct-border-default)" }
        : { background: "var(--ct-bg-active)", color: "var(--ct-muted)", border: "1px solid var(--ct-border-default)" };
  return (
    <span
      style={{
        fontSize: 9.5,
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        letterSpacing: 0,
        textTransform: "uppercase",
        borderRadius: 4,
        padding: "1px 5px",
        flex: "none",
        ...style,
      }}
    >
      {children}
    </span>
  );
}
