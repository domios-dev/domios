import { invoke } from "./bridge";
import { baseDaemonTransport } from "./daemonBootstrap";
import type { PluginSnapshot } from "../components/Extensions/types";
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

/** A local document's consent and operation use the same daemon, independent of mesh selection. */
export async function invokeViewWithConsent<T>(pluginId: string, operation: () => Promise<T>, isCurrent: () => boolean): Promise<T> {
  const client = baseDaemonTransport();
  const snapshot = await client.localJson<PluginSnapshot>("GET", "/api/plugins/snapshot");
  const plugin = snapshot.plugins.find(item => item.id === pluginId) ?? {id: pluginId};
  return runWithConsent(() => {
    if (!isCurrent()) throw new Error("Plugin action was cancelled.");
    return operation();
  }, plugin, requestPluginConsent, async request => {
    if (!isCurrent()) throw new Error("Plugin action was cancelled.");
    await client.localJson("POST", "/api/plugins/action", {kind:"permission",pluginId:request.pluginId,permission:request.permission,allowed:true,permissionSource:"action"});
  });
}
