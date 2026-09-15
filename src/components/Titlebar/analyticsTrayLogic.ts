export const ANALYTICS_PLUGIN_ID = "analytics";

export interface AnalyticsPluginRow {
  id: string;
  capabilities?: { view?: boolean } | null;
}

/** The entry exists only while the core plugin is installed with its view, so it never opens an empty frame. */
export function isAnalyticsAvailable(plugins: readonly AnalyticsPluginRow[]): boolean {
  return plugins.some((plugin) => plugin.id === ANALYTICS_PLUGIN_ID && plugin.capabilities?.view === true);
}
