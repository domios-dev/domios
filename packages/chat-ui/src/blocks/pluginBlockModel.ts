export type PluginLifecycleKind = "install" | "update";

/** Facts the host reports about one plugin; the block derives everything it shows from these. */
export interface PluginBlockInfo {
  id: string;
  name: string;
  pitch: string;
  installed: boolean;
  updateAvailable: boolean;
  compatible: boolean;
  blockedReason?: string | null;
  /** Bundled or workspace plugin: Domios manages it, so the card never offers install or update. */
  managed: boolean;
  hasView: boolean;
  pendingReload: boolean;
  version?: string | null;
  remoteVersion?: string | null;
  iconColor?: string | null;
}

export type PluginBlockState =
  | { kind: "loading" }
  | { kind: "unavailable"; message: string }
  | { kind: "missing" }
  | { kind: "ready"; plugin: PluginBlockInfo };

export type PluginStatus = "installed" | "update" | "not-installed";

export interface PluginPrimaryAction {
  kind: PluginLifecycleKind | "open" | "none";
  label: string;
  disabled: boolean;
  reason?: string;
}

const PLUGIN_ID = /^[a-z0-9][a-z0-9._-]{0,63}$/;

export function parsePluginId(argument: string): string | null {
  const id = argument.trim();
  return PLUGIN_ID.test(id) && !id.includes("..") ? id : null;
}

export function pluginStatus(plugin: PluginBlockInfo): PluginStatus {
  if (!plugin.installed) return "not-installed";
  return plugin.updateAvailable && !plugin.managed ? "update" : "installed";
}

export const PLUGIN_STATUS_LABEL: Record<PluginStatus, string> = {
  installed: "Installed",
  update: "Update available",
  "not-installed": "Not installed",
};

const INCOMPATIBLE = "Needs a newer Domios version";
const RESTART = "Restart Domios to activate it";

export function pluginPrimaryAction(plugin: PluginBlockInfo): PluginPrimaryAction {
  const blocked = plugin.compatible ? undefined : plugin.blockedReason || INCOMPATIBLE;
  switch (pluginStatus(plugin)) {
    case "not-installed":
      return { kind: "install", label: "Install", disabled: !!blocked, reason: blocked };
    case "update":
      return { kind: "update", label: "Update", disabled: !!blocked, reason: blocked };
    case "installed":
      if (plugin.pendingReload) return { kind: "none", label: "Open", disabled: true, reason: RESTART };
      return plugin.hasView ? { kind: "open", label: "Open", disabled: false } : { kind: "none", label: "", disabled: true };
  }
}

/** The in-place result line after a lifecycle click, judged against the state the host reports afterwards. */
export function pluginLifecycleResult(kind: PluginLifecycleKind, after: PluginBlockState): string {
  const verb = kind === "install" ? "Installed" : "Updated";
  if (after.kind !== "ready" || !after.plugin.installed) return `${verb}, but Domios does not list it yet. Open it in Plugins to check.`;
  return after.plugin.pendingReload ? `${verb}. ${RESTART}.` : `${verb}.`;
}
