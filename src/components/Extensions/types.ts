// (pluginCatalog.ts) can be unit-tested without the React/SVG import chain.
import type { SettingField } from "../../lib/types";

export interface PluginCapabilities {
  agent_provider?: boolean;
  secret_backend?: boolean;
  transcriber?: boolean;
  glance_view?: boolean;
  status_bubble?: boolean;
  view?: boolean;
  /** Drives a non-PTY chat tab (e.g. LM Studio). Surfaces an "Open chat" action. */
  chat_backend?: boolean;
  /** Send-time outbound text stage (`chatPreprocess`). */
  chat_preprocessor?: boolean;
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
  /** Manifest flag: the capability can't hot-activate (§1.8). */
  requires_reload?: boolean | null;
  /** Live signal: installed after daemon start, waiting on a reload to activate. */
  pending_reload?: boolean | null;
  icon_svg?: string | null;
  icon_color?: string | null;
  description?: string | null;
  /** Opt-in agent-facing instructions. Absent means AI configuration is unsupported. */
  config_help?: string | null;
  homepage?: string | null;
  readme_markdown?: string | null;
  source?: PluginInstallSource | null;
}

export interface ChannelRecord {
  name: string;
  url: string;
  path?: string;
  kind: "local" | "git";
  system?: boolean;
  removable?: boolean;
  version?: string | null;
  verified?: boolean | null;
  trusted?: boolean | null;
  last_refresh?: string | number | null;
  last_refreshed?: string | number | null;
  refreshed_at?: string | number | null;
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
  description?: string | null;
  config_help?: string | null;
  homepage?: string | null;
  readme_markdown?: string | null;
}

export interface PluginSnapshot {
  plugins: InstalledPlugin[];
  channels: ChannelRecord[];
  diff: DiffRow[];
  marketplace?: MarketplaceRow[];
}
