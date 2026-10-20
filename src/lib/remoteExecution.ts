import type {ProviderDescriptor, Tab} from "./types";

export type ExecutionLocation = "local" | "cloud";

export function atomicSpawnPreset(provider: Pick<ProviderDescriptor, "remote_task"> | undefined, preset?: string, executionMode?: string): {preset?: string; config?: {preset?: string; executionMode: string}} {
  if (!executionMode || executionMode === "local") return {preset};
  if (provider?.remote_task?.mode !== executionMode) throw new Error("This provider does not support the requested execution mode");
  return {config: {preset, executionMode}};
}

export function remoteProvider(providers: ProviderDescriptor[], providerId: string): ProviderDescriptor | undefined {
  return providers.find(provider => provider.id === providerId && provider.remote_task);
}

export function remoteSpawnSelection(providers: ProviderDescriptor[], providerId: string, environmentId: string): {providerId: string; preset: string; executionMode: string} {
  const provider = remoteProvider(providers, providerId);
  const capability = provider?.remote_task;
  if (!provider || !capability || !capability.mode.trim() || capability.mode === "local") throw new Error("This provider does not declare a Cloud execution mode");
  if (capability.availability.status === "unavailable") throw new Error(capability.availability.reason);
  if (!capability.environments.some(environment => environment.id === environmentId)) throw new Error("Choose an available Cloud environment");
  return {providerId: provider.id, preset: environmentId, executionMode: capability.mode};
}

export function isRemoteTaskTab(tab: Pick<Tab, "backing" | "agent_runtime_config">, capability: ProviderDescriptor["remote_task"]): boolean {
  return tab.backing?.kind === "chat_backend" && Boolean(capability) && tab.agent_runtime_config?.executionMode === capability?.mode;
}
