import { useEffect } from "react";
import { type AccountMeshRefusalNotice, raiseMeshMachineLimit } from "../billing/accountMeshLimit";
import { type AccountMeshClient, type AccountMeshSnapshot, bridgeAccountMeshClient } from "./accountMeshClient";

/** The refusal an unacknowledged automatic-enrollment notice carries, if any. */
export function pendingLimitNotice(
  snapshot: Pick<AccountMeshSnapshot, "limit_notice"> | null | undefined,
): AccountMeshRefusalNotice | null {
  const notice = snapshot?.limit_notice;
  if (!notice || typeof notice.request_id !== "string" || !notice.request_id) return null;
  return { requestId: notice.request_id, refusal: notice.refusal };
}

/** Opens the plans modal for a held machine-slot refusal once, then acknowledges it. */
export async function deliverLimitNotice(client: AccountMeshClient): Promise<boolean> {
  const notice = pendingLimitNotice(await client.snapshot());
  if (!notice) return false;
  raiseMeshMachineLimit(notice);
  await client.acknowledgeLimit(notice.requestId);
  return true;
}

/** App-level: refusals from automatic enrollment reach the modal even when no Mesh view is mounted. */
export function useAccountMeshLimitNotice(client: AccountMeshClient = bridgeAccountMeshClient): void {
  useEffect(() => {
    let active = true;
    const deliver = () => {
      if (active) void deliverLimitNotice(client).catch(() => undefined);
    };
    const unsubscribe = client.subscribe(deliver);
    deliver();
    return () => {
      active = false;
      unsubscribe();
    };
  }, [client]);
}
