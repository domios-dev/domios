import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "../../../lib/bridge";
import type { DocumentFileCheck } from "../chatDocumentsModel";

const homes = new Map<string, string | null>();
const requests = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();

function load(tabId: string): Promise<void> {
  const existing = requests.get(tabId);
  if (existing) return existing;
  const request = invoke<DocumentFileCheck>("check_local_preview_files", { paths: [], tabId }).then((result) => {
    homes.set(tabId, result?.home ?? null);
    for (const listener of listeners) listener();
  }, () => undefined);
  requests.set(tabId, request);
  return request;
}

/** The user's home directory for `~` paths; null until the host answers or when it cannot. */
export function useHomeDirectory(tabId: string): string | null {
  useEffect(() => { void load(tabId); }, [tabId]);
  return useSyncExternalStore((listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => homes.get(tabId) ?? null, () => null);
}
