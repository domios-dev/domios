import type {ProviderDescriptor} from "./types";

export type ExecutionLocation = "local" | "remote";

export function atomicSpawnPreset(provider: Pick<ProviderDescriptor, "remote_task"> | undefined, preset?: string): {preset?: string; config?: {preset?: string}} {
  return provider?.remote_task ? {config: {preset}} : {preset};
}

export function remoteProvider(providers: ProviderDescriptor[], localId: string): ProviderDescriptor | undefined {
  const candidates = providers.filter(provider => provider.transport === "chat_backend" && provider.remote_task?.localProviderId === localId);
  return candidates.length === 1 ? candidates[0] : undefined;
}

export function remoteSpawnSelection(providers: ProviderDescriptor[], localId: string, environmentId: string): {providerId: string; preset: string} {
  const provider = remoteProvider(providers, localId);
  const capability = provider?.remote_task;
  if (!provider || !capability) throw new Error("This provider does not declare a remote task backend");
  if (capability.availability.status === "unavailable") throw new Error(capability.availability.reason);
  if (!capability.environments.some(environment => environment.id === environmentId)) {
    throw new Error("Choose an available remote environment");
  }
  return {providerId: provider.id, preset: environmentId};
}
