import { useCallback, useEffect, useRef, useState } from "react";
import { openOnMeshRefusal, raiseMeshMachineLimit } from "../billing/accountMeshLimit";
import { settledLinks, type MachineRowView } from "./accountMachines";
import { linkFailure, type AccountMeshClient, type AccountMeshFailure, type AccountMeshSnapshot, type MeshTarget } from "./accountMeshClient";

interface Options {
  client: AccountMeshClient;
  snapshot: AccountMeshSnapshot | null;
  load: (refreshRegistry?: boolean) => Promise<void>;
  onChanged?: () => void;
  fail: (key: string, operation: string, failure: AccountMeshFailure) => void;
  dismissFailure: (key: string) => void;
}

export function useMachineLinkActions({ client, snapshot, load, onChanged, fail, dismissFailure }: Options) {
  const [linking, setLinking] = useState<ReadonlySet<string>>(() => new Set());
  const previousRequests = useRef(new Map<string, string | undefined>());
  const reported = useRef(new Set<string>());
  const [deciding, setDeciding] = useState<string | null>(null);

  useEffect(() => {
    if (!snapshot) return;
    if (snapshot.traffic !== "open" || snapshot.binding !== "current") {
      if (linking.size > 0) setLinking(new Set());
      previousRequests.current.clear();
      return;
    }
    const settled = settledLinks(linking, snapshot).filter((id) => snapshot.pairs.some((pair) => pair.installation_id === id) || snapshot.link_requests?.some((request) => !request.incoming && request.installation_id === id && request.request_id !== previousRequests.current.get(id)));
    if (settled.length > 0) setLinking((current) => new Set([...current].filter((id) => !settled.includes(id))));
    for (const id of settled) previousRequests.current.delete(id);
    for (const request of snapshot.link_requests ?? []) {
      if (request.incoming || !request.failure || reported.current.has(request.request_id)) continue;
      reported.current.add(request.request_id);
      if (reported.current.size > 64) reported.current.delete(reported.current.values().next().value!);
      if (request.failure.reason === "mesh.machines") {
        raiseMeshMachineLimit({ requestId: request.request_id, refusal: request.failure });
      } else if (request.decision === "failed") {
        const name = snapshot.registry?.rows.find((row) => row.installation_id === request.installation_id)?.display_name ?? "machine";
        fail(request.installation_id, `connect ${name}`, request.failure);
      }
    }
  }, [snapshot, linking, fail]);

  const connect = useCallback(async (row: MachineRowView, target: MeshTarget) => {
    dismissFailure(row.key);
    previousRequests.current.set(target.installation_id, snapshot?.link_requests?.find((request) => !request.incoming && request.installation_id === target.installation_id)?.request_id);
    setLinking((current) => new Set(current).add(target.installation_id));
    let failure: AccountMeshFailure | null = null;
    let planLimit = false;
    try {
      planLimit = await openOnMeshRefusal(async () => { await client.link(target); return true; }) === undefined;
    } catch (error) {
      failure = linkFailure(error);
    }
    if (failure || planLimit) setLinking((current) => new Set([...current].filter((id) => id !== target.installation_id)));
    if (failure) fail(row.key, `connect ${row.name}`, failure);
    onChanged?.();
    await load(false);
  }, [client, dismissFailure, fail, load, onChanged, snapshot]);

  const decide = useCallback(async (row: MachineRowView, notificationId: string, approve: boolean) => {
    setDeciding(row.key);
    try { await client.answerLink(notificationId, approve); }
    catch { fail(row.key, `answer ${row.name}`, { reason: "unavailable" }); }
    finally { setDeciding(null); }
    await load(false);
    onChanged?.();
  }, [client, fail, load, onChanged]);

  return { linking, connect, decide, deciding };
}
