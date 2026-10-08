import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "../../../lib/bridge";
import type { DocumentFileCheck } from "../chatDocumentsModel";

let home: string | null = null;
let request: Promise<void> | null = null;
const listeners = new Set<() => void>();

function load(): Promise<void> {
  request ??= invoke<DocumentFileCheck>("check_local_preview_files", { paths: [] }).then((result) => {
    home = result?.home ?? null;
    for (const listener of listeners) listener();
  }, () => undefined);
  return request;
}

/** The user's home directory for `~` paths; null until the host answers or when it cannot. */
export function useHomeDirectory(): string | null {
  useEffect(() => { void load(); }, []);
  return useSyncExternalStore((listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => home, () => null);
}
