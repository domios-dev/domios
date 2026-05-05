/**
 * Live list of agent providers contributed by active plugins.
 *
 * Single source of truth: the backend's `list_agent_providers` Tauri command,
 * which calls `dispatch::Command::ListAgentProviders` → `AgentRegistry::list_descriptors()`.
 * Auto-refetches when a `contributions-changed { kind: "agent_provider" }` event
 * arrives (plugin activated/deactivated).
 *
 * Adding a new bundled or third-party agent plugin requires ZERO changes here —
 * the descriptor flows through automatically.
 */
import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { ProviderDescriptor } from "../lib/types";
import { setAgentDescriptors } from "../lib/agentDescriptorCache";
import { useContributions } from "./useContributions";

async function fetchAgentProviders(): Promise<ProviderDescriptor[]> {
  const result = await invoke<ProviderDescriptor[]>("list_agent_providers");
  return result ?? [];
}

export function useAgentProviders(): {
  providers: ProviderDescriptor[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
} {
  const fetcher = useCallback(fetchAgentProviders, []);
  const { items, loading, error, refetch } = useContributions<ProviderDescriptor>(
    "agent_provider",
    fetcher,
  );
  useEffect(() => {
    setAgentDescriptors(items);
  }, [items]);
  return { providers: items, loading, error, refetch };
}
