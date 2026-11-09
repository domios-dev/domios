import type { SensitivePermission } from "../../lib/pluginConsentModel";

type OptionalFields<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;
type WireInstalled = import("../../../gen/InstalledPlugin").InstalledPlugin;
type WireMarketplace = import("../../../gen/MarketplaceRow").MarketplaceRow;
type SnapshotUpdate = Pick<import("../../../gen/PluginUpdate").PluginUpdate, "available" | "reason"> & Partial<Omit<import("../../../gen/PluginUpdate").PluginUpdate, "available" | "reason">>;


export type PluginCapabilities = Partial<import("../../../gen/PluginSnapshotCapabilities").PluginSnapshotCapabilities>;

export type PluginInstallSource = import("../../../gen/PluginInstallSource").PluginInstallSource;

/** A plugin physically installed on disk, workspace-loaded, or a bundled system
 * provider — the daemon's `snapshot.plugins`. */
export type PluginLoadError = Pick<import("../../../gen/PluginLoadError").PluginLoadError, "message" | "path" | "at_ms" | "kept_previous">;

export type InstalledPlugin = Omit<Pick<WireInstalled, "id"> & Partial<Omit<WireInstalled, "id">>, "capabilities" | "sensitive_permissions" | "update" | "load_error" | "version" | "display_name"> & { capabilities?: PluginCapabilities | null; sensitive_permissions?: SensitivePermission[] | null; update?: SnapshotUpdate; load_error?: PluginLoadError | null; version?: WireInstalled["version"] | null; display_name?: WireInstalled["display_name"] | null };

export type ChannelRecord = OptionalFields<import("../../../gen/PluginChannelRecord").PluginChannelRecord, "path" | "system" | "removable">;

/** Channel diff row — the raw `snapshot.diff`. */
export type DiffRow = Pick<import("../../../gen/PluginDiffRow").PluginDiffRow, "id" | "channel" | "drift"> & Partial<Omit<import("../../../gen/PluginDiffRow").PluginDiffRow, "id" | "channel" | "drift">>;

/** Marketplace row — `snapshot.marketplace`: a diff row enriched with the
 * channel-cache manifest metadata and an honest `installed` flag. */
export type MarketplaceRow = Omit<Pick<WireMarketplace, "id" | "channel" | "drift"> & Partial<Omit<WireMarketplace, "id" | "channel" | "drift">>, "capabilities" | "sensitive_permissions"> & { capabilities?: PluginCapabilities | null; sensitive_permissions?: SensitivePermission[] | null };

export type PluginSnapshot = Partial<Omit<import("../../../gen/PluginSnapshot").PluginSnapshot, "plugins" | "channels" | "diff" | "marketplace">> & { plugins: InstalledPlugin[]; channels: ChannelRecord[]; diff: DiffRow[]; marketplace?: MarketplaceRow[] };
