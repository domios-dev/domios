import { useEffect, useState } from "react";
import type { NeighborPermission, NeighborRuntimeClient } from "./neighborRuntimeClient";

export interface ReceiverGrantDecision { choice: "keep" | "adopt"; expected_digest: string; scope?: string }
export interface ReceiverGrantPreview {
  revision: number;
  previous_machine: string | null;
  local: NeighborPermission;
  previous: NeighborPermission | null;
  different: boolean;
  digest: string;
  unavailable: boolean;
}

export function grantSummary(grant: NeighborPermission): string {
  return `${grant.allowed ? "Enabled" : "Disabled"} · Machine: ${grant.machine_access} · Folder: ${grant.effective_folder || grant.folder || "friend's default folder"} · Writes: ${grant.folder_enforcement.writes} · Memory reads: ${grant.allow_memory ? (grant.memory_areas?.join(", ") ?? "all non-secret areas") || "none" : "off"} · Memory writes: ${grant.memory_write_areas.join(", ") || "none"} · Workspaces: ${grant.workspaces?.join(", ") || "none"} · Tab requests: ${grant.allow_tab_requests ? "on" : "off"} · Files: ${grant.allow_files ? "on" : "off"} · Interactive content: ${grant.allow_interactive_content ? "on" : "off"}`;
}

export function useReceiverGrants(client: Pick<NeighborRuntimeClient, "receiverGrants">, friendship: string, revision: number, enabled: boolean) {
  const [result, setResult] = useState<{ friendship: string; revision: number; preview?: ReceiverGrantPreview; error?: string } | null>(null);
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    void client.receiverGrants(friendship, controller.signal).then(preview => {
      if (!controller.signal.aborted) setResult({ friendship, revision, preview });
    }, (error: unknown) => {
      if (!controller.signal.aborted) setResult({ friendship, revision, error: error instanceof Error ? error.message : "Could not read receiver permissions" });
    });
    return () => controller.abort();
  }, [client, friendship, revision, enabled]);
  return result?.friendship === friendship && result.revision === revision ? result : null;
}
