import { invoke } from "./bridge";
import { ensurePluginSnapshot, runPluginSnapshotAction } from "./pluginSnapshotStore";
import { runWithConsent, type GrantRequest } from "./pluginConsentModel";
import { requestPluginConsent } from "./pluginConsentStore";

export async function setPluginPermission(pluginId: string, permission: string, allowed: boolean) {
  await runPluginSnapshotAction({ kind: "permission", pluginId, permission, allowed });
}

async function grantPermission(request: GrantRequest) {
  await runPluginSnapshotAction({ kind: "permission", pluginId: request.pluginId, permission: request.permission, allowed: true, permissionSource: "action" });
}

export async function invokePluginWithConsent<T>(command: string, args: Record<string, unknown> & { pluginId: string }, isCurrent: () => boolean = () => true): Promise<T> {
  const snapshot = await ensurePluginSnapshot();
  const plugin = snapshot.plugins.find((item) => item.id === args.pluginId) ?? { id: args.pluginId };
  return runWithConsent(() => {
    if (!isCurrent()) throw new Error("Plugin action was cancelled.");
    return invoke<T>(command, args);
  }, plugin, requestPluginConsent, grantPermission);
}
