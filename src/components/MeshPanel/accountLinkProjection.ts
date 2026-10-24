import type { AccountLinkRequest } from "./accountMeshClient";
import type { MachineRowView } from "./accountMachines";

export function projectLinkRequest(row: MachineRowView, request: AccountLinkRequest | undefined): MachineRowView {
  if (!request || ["revoked", "signed_out", "over_limit", "blocked", "other_account", "unverified"].includes(row.status)) return row;
  if (request.incoming) {
    if (request.decision !== "waiting") return row;
    return { ...row, linkRequest: request, status: "approval_requested", detail: `${row.name} wants to connect`, controls: { ...row.controls, add: null } };
  }
  if (row.pairInstanceId) return row;
  if (request.decision === "waiting") {
    return { ...row, linkRequest: request, status: "linking", detail: `Waiting for approval on ${row.name}`, controls: { ...row.controls, add: { reason: null } } };
  }
  const status = request.decision === "expired" ? "link_expired" : request.decision === "declined" ? "declined" : "link_failed";
  const detail = status === "link_expired" ? "Expired" : status === "declined" ? "Declined" : "Connection failed";
  return { ...row, linkRequest: request, status, detail, controls: { ...row.controls, add: { reason: null } } };
}

export function projectLinkRequests(rows: MachineRowView[], requests: readonly AccountLinkRequest[]): MachineRowView[] {
  const result = rows.map((row) => projectLinkRequest(row, requests.find((request) => request.installation_id === row.key)));
  for (const request of requests) {
    if (!request.incoming || request.decision !== "waiting" || result.some((row) => row.key === request.installation_id)) continue;
    result.push({ key: request.installation_id, name: request.name, status: "approval_requested", detail: `${request.name} wants to connect`,
      online: true, target: null, pairInstanceId: null, lastSeenAt: null, linkRequest: request,
      controls: { add: null, revoke: null, unpairInstanceId: null } });
  }
  return result;
}
