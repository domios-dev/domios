import { createContext, useContext } from "react";
import { invoke, listen } from "../../lib/bridge";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { AccountMeshRefusalError } from "../billing/accountMeshLimit";

/** Exact registry slot identity the server checks before revoke or link. */
export interface MeshTarget {
  installation_id: string;
  key_id: string;
  key_generation: number;
  slot_revision: number;
}

export interface MeshEndpointObservation {
  tailnet_ip: string;
  api_port: number;
  expires_at: number;
}

/** One account registry row as the server lists it; `endpoint` is published metadata, never reachability. */
export interface AccountMachineRow extends MeshTarget {
  active: boolean;
  presence: "present" | "signed_out";
  admitted_at: number;
  authenticated_at: number;
  display_name: string | null;
  metadata_revision: number;
  publication: "none" | "pending" | "complete";
  membership_expires_at: number | null;
  eligibility: "eligible" | "mesh-allowance" | "revoked" | "verification-unavailable";
  endpoint: MeshEndpointObservation | null;
}

export interface SelfInstallation extends MeshTarget {
  metadata_revision: number;
  display_name: string | null;
}

/** A locally saved pairing; `installation_id` is null until the pair is joined to an account machine. */
export interface LocalPairFact {
  instance_id: string;
  installation_id: string | null;
  admission: "legacy" | "adopted";
  online: boolean;
  name: string;
}

export type AccountMeshFailure =
  | { reason: "invalid" | "unauthenticated" | "app-check" | "forbidden" | "conflict" | "authority-unavailable" | "unavailable" }
  | { reason: "rate-limited"; retry_after_seconds: number }
  | { reason: "mesh.machines"; limit: number; active_count: number }
  | { reason: "offline" | "peer-unreachable" | "handshake-failed" | "signed-out" | "declined" | "expired" }
  | { reason: "revoked" | "cancelled" | "verification-unavailable" | "plan_limit_locked" | "reauth_required" };

export type AccountMeshOutcome = { ok: true } | { ok: false; failure: AccountMeshFailure };

/** A genuine free machine-slot refusal from automatic enrollment, held until acknowledged. */
export interface AccountMeshLimitNotice {
  request_id: string;
  refusal: unknown;
  trigger: string;
}

export interface AccountLinkRequest {
  request_id: string;
  installation_id: string;
  name: string;
  incoming: boolean;
  expires_at: number;
  decision: "waiting" | "approved" | "declined" | "expired" | "failed";
  notification_id: string | null;
  failure: AccountMeshFailure | null;
}

export interface AccountMeshSnapshot {
  link_requests?: AccountLinkRequest[];
  binding: "none" | "current" | "other";
  traffic: "open" | "blocked_signed_out" | "plan_limit_locked";
  cleanup: "none" | "pending" | "confirmed";
  self: SelfInstallation | null;
  registry: { observed_at: number; rows: AccountMachineRow[] } | null;
  registry_failure: AccountMeshFailure | null;
  enrollment: "idle" | "enrolling" | "enrolled" | "refused";
  last_refusal: AccountMeshFailure | null;
  publication?: "none" | "not_ready" | "published";
  publication_failure?: AccountMeshFailure | null;
  retry_at?: number | null;
  pairs: LocalPairFact[];
  limit_notice?: AccountMeshLimitNotice | null;
}

type LinkResult = { ok: true } | { ok: false; request_id: string; failure: AccountMeshFailure };

export interface AccountMeshClient {
  snapshot(): Promise<AccountMeshSnapshot>;
  enroll(): Promise<AccountMeshOutcome>;
  list(): Promise<AccountMeshOutcome>;
  subscribe(onChange: () => void): () => void;
  /** Resolves when the request is accepted; the snapshot carries approval and the final outcome. */
  link(target: MeshTarget): Promise<void>;
  answerLink(notificationId: string, approve: boolean): Promise<void>;
  renameSelf(expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome>;
  renamePeer?(instanceId: string, expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome>;
  revoke(target: MeshTarget): Promise<AccountMeshOutcome>;
  unpair(instanceId: string): Promise<void>;
  acknowledgeLimit(requestId: string): Promise<void>;
}

const COMMANDS = {
  state: "account_mesh_state",
  enroll: "account_mesh_enroll",
  list: "account_mesh_list",
  link: "account_mesh_link",
  renameSelf: "account_mesh_rename_self",
  revoke: "account_mesh_revoke",
  acknowledgeLimit: "account_mesh_acknowledge_limit",
} as const;
const CHANGE_EVENTS = ["account-changed", "account-mesh-changed", "mesh-state-changed"] as const;

export const bridgeAccountMeshClient: AccountMeshClient = {
  snapshot: () => invoke<AccountMeshSnapshot>(COMMANDS.state),
  enroll: () => invoke<AccountMeshOutcome>(COMMANDS.enroll),
  list: () => invoke<AccountMeshOutcome>(COMMANDS.list),
  subscribe(onChange) {
    let active = true;
    const unlisteners: Array<() => void> = [];
    for (const event of CHANGE_EVENTS) {
      void listen(event, () => { if (active) onChange(); })
        .then((unlisten) => { if (active) unlisteners.push(unlisten); else unlisten(); })
        .catch(() => undefined);
    }
    window.addEventListener("domios:account-changed", onChange);
    return () => {
      active = false;
      unlisteners.forEach((unlisten) => unlisten());
      window.removeEventListener("domios:account-changed", onChange);
    };
  },
  async link(target) {
    const result = await invoke<LinkResult>(COMMANDS.link, { installationId: target.installation_id });
    if (!result.ok) throw new AccountMeshRefusalError({ requestId: result.request_id, refusal: result.failure });
  },
  answerLink: async (notificationId, approve) => { await invoke("answer_notification", { id: notificationId, label: approve ? "Approve" : "Decline" }); },
  renameSelf: (expectedMetadataRevision, displayName) =>
    invoke<AccountMeshOutcome>(COMMANDS.renameSelf, { expectedMetadataRevision, displayName }),
  renamePeer: (instanceId, expectedMetadataRevision, displayName) => daemonTransportForPeer(instanceId).json("POST", "/api/account/mesh/rename", {
    expected_metadata_revision: expectedMetadataRevision, display_name: displayName,
  }),
  revoke: (target) => invoke<AccountMeshOutcome>(COMMANDS.revoke, { target }),
  unpair: async (instanceId) => { await invoke("mesh_disconnect", { instanceId }); },
  acknowledgeLimit: async (requestId) => { await invoke(COMMANDS.acknowledgeLimit, { requestId }); },
};

const AccountMeshClientContext = createContext<AccountMeshClient>(bridgeAccountMeshClient);
export const AccountMeshClientProvider = AccountMeshClientContext.Provider;
export const useAccountMeshClient = () => useContext(AccountMeshClientContext);

const FAILURE_REASONS = new Set<string>([
  "invalid", "unauthenticated", "app-check", "forbidden", "conflict", "authority-unavailable", "unavailable",
  "rate-limited", "mesh.machines", "offline", "peer-unreachable", "handshake-failed", "signed-out",
  "declined", "expired", "revoked", "cancelled", "verification-unavailable", "reauth_required",
]);

/** The typed failure a rejected link carries; anything unrecognized is reported as unavailable. */
export function linkFailure(error: unknown): AccountMeshFailure {
  if (!(error instanceof AccountMeshRefusalError)) return { reason: "unavailable" };
  const refusal = error.notice.refusal as { reason?: unknown } | null;
  return refusal && typeof refusal.reason === "string" && FAILURE_REASONS.has(refusal.reason)
    ? refusal as AccountMeshFailure
    : { reason: "unavailable" };
}
