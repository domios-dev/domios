// Shared types for the Extensions surface, kept view-free so the catalog logic
// (pluginCatalog.ts) can be unit-tested without the React/SVG import chain.
import type { SettingField } from "../../lib/types";

export interface PluginCapabilities {
  agent_provider?: boolean;
  secret_backend?: boolean;
  transcriber?: boolean;
  glance_view?: boolean;
  status_bubble?: boolean;
  view?: boolean;
}

export type PluginInstallSource =
  | { kind: "dir"; path: string }
  | { kind: "channel"; channel: string; path: string }
  | { kind: "system_channel"; channel: string; path: string };

/** A plugin physically installed on disk, workspace-loaded, or a bundled system
 * provider — the daemon's `snapshot.plugins`. */
export interface InstalledPlugin {
  id: string;
  version?: string | null;
  display_name?: string | null;
  state?: string;
  system?: boolean;
  removable?: boolean;
  verified?: boolean;
  /** Loaded from the workspace `plugins/` source tree (dev), not a marketplace install. */
  dev?: boolean;
  capabilities?: PluginCapabilities | null;
  granted_permissions?: string[] | null;
  settings_schema?: SettingField[] | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
  description?: string | null;
  homepage?: string | null;
  readme_markdown?: string | null;
  source?: PluginInstallSource | null;
}

export interface ChannelRecord {
  name: string;
  url: string;
  kind: "local" | "git";
  system?: boolean;
  removable?: boolean;
}

/** Channel diff row — the raw `snapshot.diff`. */
export interface DiffRow {
  channel: string;
  id: string;
  local_version?: string | null;
  remote_version?: string | null;
  drift: string;
  compatible?: boolean;
  blocked_reason?: string | null;
}

/** Marketplace row — `snapshot.marketplace`: a diff row enriched with the
 * channel-cache manifest metadata and an honest `installed` flag. */
export interface MarketplaceRow extends DiffRow {
  installed?: boolean;
  display_name?: string | null;
  capabilities?: PluginCapabilities | null;
  granted_permissions?: string[] | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
  description?: string | null;
  homepage?: string | null;
  readme_markdown?: string | null;
}

export interface PluginSnapshot {
  plugins: InstalledPlugin[];
  channels: ChannelRecord[];
  diff: DiffRow[];
  marketplace?: MarketplaceRow[];
}
