// Left column of the Extensions surface: search box, filter tabs, and the
// plugin rows (installed + marketplace) with status/update/installed badges.
import type { CatalogEntry, CatalogFilter } from "./pluginCatalog";
import { catalogKindLabel } from "./pluginCatalog";
import { PluginIcon } from "./PluginIcon";
import { iconOf } from "./shared";
import { PinIcon } from "../Icons";

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
}) {
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
        {entries.map((e) => (
          <PluginRow
            key={e.id}
            entry={e}
            selected={e.id === selectedId}
            pinned={pinned.has(e.id)}
            onSelect={() => onSelect(e.id)}
            onTogglePin={() => onTogglePin(e.id)}
          />
        ))}
        {entries.length === 0 && (
          <div style={{ color: "var(--ct-muted)", fontSize: 12.5, padding: "8px 4px" }}>No extensions match.</div>
        )}
      </div>
    </div>
  );
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
      <div style={{ width: 26, height: 26, borderRadius: 6, display: "flex", alignItems: "center", justifyContent: "center", flex: "none", background: "rgba(255,255,255,0.04)" }}>
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
      ? { background: "transparent", color: "var(--ct-green, #22c55e)", border: "1px solid var(--ct-green, #22c55e)" }
      : tone === "dev"
        ? { background: "transparent", color: "var(--ct-accent, #6366f1)", border: "1px solid var(--ct-accent, #6366f1)" }
        : { background: "var(--ct-bg-active)", color: "var(--ct-muted)", border: "1px solid var(--ct-border-default)" };
  return (
    <span
      style={{
        fontSize: 9.5,
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        letterSpacing: "0.04em",
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
