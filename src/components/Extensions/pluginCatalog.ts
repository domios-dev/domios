// Pure catalog logic for the Extensions surface: merge the installed inventory
// and the channel marketplace into one normalized list, then search/filter it.
// View-free so it can be unit-tested with plain tsx (no React import chain).
import type {
  InstalledPlugin,
  MarketplaceRow,
  PluginCapabilities,
  PluginSnapshot,
} from "./types";
import type { SettingField } from "../../lib/types";

export type CatalogFilter = "all" | "installed" | "marketplace" | "updates" | "channels";

export interface CatalogEntry {
  id: string;
  displayName: string;
  installed: boolean;
  /** Present in at least one registered channel. */
  inMarketplace: boolean;
  system: boolean;
  removable: boolean;
  /** Loaded from the workspace source tree (dev), not a marketplace install. */
  dev: boolean;
  version?: string | null; // installed / local version
  remoteVersion?: string | null; // channel version
  channel?: string | null;
  drift?: string | null;
  compatible: boolean;
  blockedReason?: string | null;
  updateAvailable: boolean;
  /** Installed after daemon start, capability waiting on a reload to activate (§1.8). */
  pendingReload?: boolean | null;
  capabilities?: PluginCapabilities | null;
  grantedPermissions?: string[] | null;
  settingsSchema?: SettingField[] | null;
  description?: string | null;
  /** Agent-facing help for the "Configure with AI" button (Part D). */
  configHelp?: string | null;
  homepage?: string | null;
  readmeMarkdown?: string | null;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  source?: InstalledPlugin["source"] | null;
}

export type PrimaryActionKind = "install" | "update" | "uninstall" | "none";

export interface PrimaryAction {
  kind: PrimaryActionKind;
  label: string;
  disabled: boolean;
  reason?: string;
}

const UPDATE_DRIFTS = new Set(["update_available", "hash_drift"]);

function caps(c?: PluginCapabilities | null): PluginCapabilities {
  return c ?? {};
}

function kindOf(entry: CatalogEntry): string {
  if (entry.system) return "system";
  const c = caps(entry.capabilities);
  if (c.secret_backend) return "secret backend";
  if (c.transcriber) return "transcriber";
  if (c.agent_provider) return "agent";
  return "plugin";
}

/** Normalized one-line kind label for a catalog entry. */
export function catalogKindLabel(entry: CatalogEntry): string {
  return kindOf(entry);
}

/** Capability flags that are true, as a stable string list (for search/chips). */
export function capabilityKeys(c?: PluginCapabilities | null): string[] {
  const obj = caps(c);
  return Object.entries(obj)
    .filter(([, v]) => v === true)
    .map(([k]) => k);
}

function entryFromInstalled(p: InstalledPlugin): CatalogEntry {
  const sourceChannel = p.source?.kind === "channel" || p.source?.kind === "system_channel" ? p.source.channel : null;
  return {
    id: p.id,
    displayName: p.display_name || p.id,
    installed: true,
    inMarketplace: false,
    system: p.system === true,
    removable: p.removable !== false && p.system !== true,
    dev: p.dev === true,
    version: p.version ?? null,
    remoteVersion: null,
    channel: sourceChannel,
    drift: null,
    compatible: true,
    blockedReason: null,
    updateAvailable: false,
    pendingReload: p.pending_reload ?? false,
    capabilities: p.capabilities ?? null,
    grantedPermissions: p.granted_permissions ?? null,
    settingsSchema: p.settings_schema ?? null,
    description: p.description ?? null,
    configHelp: p.config_help ?? null,
    homepage: p.homepage ?? null,
    readmeMarkdown: p.readme_markdown ?? null,
    iconSvg: p.icon_svg ?? null,
    iconColor: p.icon_color ?? null,
    iconHint: p.icon_hint ?? null,
    source: p.source ?? null,
  };
}

/** Fold a marketplace row into an existing or fresh entry. */
function mergeMarketplace(base: CatalogEntry | undefined, m: MarketplaceRow): CatalogEntry {
  const installed = base?.installed ?? m.installed === true;
  const compatible = m.compatible !== false;
  const updateAvailable = installed && UPDATE_DRIFTS.has(m.drift);
  const entry: CatalogEntry = {
    id: m.id,
    displayName: base?.displayName || m.display_name || m.id,
    installed,
    inMarketplace: true,
    system: base?.system ?? false,
    removable: base?.removable ?? false,
    dev: base?.dev ?? false,
    version: base?.version ?? m.local_version ?? null,
    remoteVersion: m.remote_version ?? null,
    channel: m.channel ?? null,
    drift: m.drift ?? null,
    compatible,
    blockedReason: m.blocked_reason ?? null,
    updateAvailable,
    pendingReload: base?.pendingReload ?? false,
    // Prefer installed-side metadata (loaded from the live install) but fall
    // back to the channel manifest for not-yet-installed plugins.
    capabilities: base?.capabilities ?? m.capabilities ?? null,
    grantedPermissions: base?.grantedPermissions ?? m.granted_permissions ?? null,
    settingsSchema: base?.settingsSchema ?? null,
    description: base?.description ?? m.description ?? null,
    configHelp: base?.configHelp ?? m.config_help ?? null,
    homepage: base?.homepage ?? m.homepage ?? null,
    readmeMarkdown: base?.readmeMarkdown ?? m.readme_markdown ?? null,
    iconSvg: base?.iconSvg ?? m.icon_svg ?? null,
    iconColor: base?.iconColor ?? m.icon_color ?? null,
    iconHint: base?.iconHint ?? m.icon_hint ?? null,
    source: base?.source ?? null,
  };
  return entry;
}

/** Merge installed inventory + channel marketplace into one keyed catalog. */
export function buildCatalog(snapshot: PluginSnapshot): CatalogEntry[] {
  const byId = new Map<string, CatalogEntry>();
  for (const p of snapshot.plugins ?? []) {
    byId.set(p.id, entryFromInstalled(p));
  }
  for (const m of snapshot.marketplace ?? []) {
    byId.set(m.id, mergeMarketplace(byId.get(m.id), m));
  }
  return [...byId.values()];
}

/** Does this entry match a free-text query (id / name / channel / capability /
 * description)? Empty query matches everything. */
export function searchMatches(entry: CatalogEntry, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const haystack = [
    entry.id,
    entry.displayName,
    entry.channel ?? "",
    entry.description ?? "",
    catalogKindLabel(entry),
    ...capabilityKeys(entry.capabilities),
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(q);
}

function matchesFilter(entry: CatalogEntry, filter: CatalogFilter): boolean {
  switch (filter) {
    case "installed":
      return entry.installed;
    case "marketplace":
      return entry.inMarketplace;
    case "updates":
      return entry.updateAvailable;
    case "channels":
      return false;
    default:
      return true;
  }
}

/** Filter + search, sorted: pinned-agnostic deterministic order (installed
 * first, then by display name) so the list is stable across refreshes. */
export function filterCatalog(
  entries: CatalogEntry[],
  query: string,
  filter: CatalogFilter,
): CatalogEntry[] {
  return entries
    .filter((e) => matchesFilter(e, filter) && searchMatches(e, query))
    .sort((a, b) => {
      if (a.installed !== b.installed) return a.installed ? -1 : 1;
      return a.displayName.localeCompare(b.displayName);
    });
}

/** Count of entries with an available update — drives the "Updates" filter badge. */
export function updateCount(entries: CatalogEntry[]): number {
  return entries.filter((e) => e.updateAvailable).length;
}

/** The one most-important action for an entry's detail pane. Uninstall is also
 * offered as a secondary action whenever `entry.removable` (see `canUninstall`). */
export function primaryAction(entry: CatalogEntry): PrimaryAction {
  if (!entry.installed) {
    if (!entry.compatible) {
      return {
        kind: "install",
        label: "Install",
        disabled: true,
        reason: entry.blockedReason ?? "Incompatible with this CodeTerm version",
      };
    }
    return { kind: "install", label: "Install", disabled: false };
  }
  if (entry.updateAvailable) {
    if (!entry.compatible) {
      return {
        kind: "update",
        label: "Update",
        disabled: true,
        reason: entry.blockedReason ?? "Incompatible with this CodeTerm version",
      };
    }
    return { kind: "update", label: "Update", disabled: false };
  }
  if (entry.dev) {
    return {
      kind: "none",
      label: "Dev plugin",
      disabled: true,
      reason: "Loaded from your workspace source tree — not a marketplace install. Remove it by deleting its folder from plugins/.",
    };
  }
  if (entry.system || !entry.removable) {
    return {
      kind: "none",
      label: entry.system ? "System plugin" : "Built in",
      disabled: true,
      reason: entry.system ? "Bundled provider — managed by CodeTerm" : undefined,
    };
  }
  return { kind: "uninstall", label: "Uninstall", disabled: false };
}

/** Whether a secondary Uninstall control should be offered (installed, removable,
 * non-system) — true even when the primary action is Update. */
export function canUninstall(entry: CatalogEntry): boolean {
  return entry.installed && entry.removable && !entry.system;
}
