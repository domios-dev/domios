import { useSyncExternalStore } from "react";
import type { ProviderDescriptor } from "./types";
import { getAgentDescriptor, subscribeAgentDescriptors } from "./agentDescriptorCache";

export function useAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  return useSyncExternalStore(
    subscribeAgentDescriptors,
    () => getAgentDescriptor(id),
    () => undefined,
  );
}
