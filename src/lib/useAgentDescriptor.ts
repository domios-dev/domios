import { useSyncExternalStore } from "react";
import type { ProviderDescriptor } from "./types";
import {
  getAgentDescriptor,
  listAgentDescriptors,
  subscribeAgentDescriptors,
} from "./agentDescriptorCache";

export function useAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  return useSyncExternalStore(
    subscribeAgentDescriptors,
    () => getAgentDescriptor(id),
    () => undefined,
  );
}

export function useAgentDescriptors(): ProviderDescriptor[] {
  return useSyncExternalStore(
    subscribeAgentDescriptors,
    listAgentDescriptors,
    listAgentDescriptors,
  );
}
