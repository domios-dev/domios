/**
 * Plugin settings seam for the Plugins UI. Reads and writes the plugin's own
 * `config.yaml` through the daemon — the file `domios plugin config` edits and
 * the host returns from `host.settingsJson()`.
 */
import { invoke } from "../../lib/bridge";

export type PluginSettings = Record<string, unknown>;

function settingsFrom(response: unknown): PluginSettings {
  const settings = (response as { settings?: unknown } | null)?.settings;
  return settings && typeof settings === "object" && !Array.isArray(settings) ? (settings as PluginSettings) : {};
}

export async function loadPluginSettings(pluginId: string): Promise<PluginSettings> {
  return settingsFrom(await invoke("plugin_manager_action", { action: { kind: "read-settings", pluginId } }));
}

export async function savePluginSettings(pluginId: string, values: PluginSettings): Promise<PluginSettings> {
  return settingsFrom(await invoke("plugin_manager_action", { action: { kind: "write-settings", pluginId, values } }));
}
