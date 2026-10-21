import { createContext, useContext } from "react";
import { invoke, listen } from "../../lib/bridge";

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
  | { reason: "offline" | "peer-unreachable" | "handshake-failed" | "signed-out" };

export type AccountMeshOutcome = { ok: true } | { ok: false; failure: AccountMeshFailure };

export interface AccountMeshSnapshot {
  binding: "none" | "current" | "other";
  traffic: "open" | "blocked_signed_out";
  cleanup: "none" | "pending" | "confirmed";
  self: SelfInstallation | null;
  registry: { observed_at: number; rows: AccountMachineRow[] } | null;
  registry_failure: AccountMeshFailure | null;
  pairs: LocalPairFact[];
}

export interface AccountMeshClient {
  snapshot(): Promise<AccountMeshSnapshot>;
  subscribe(onChange: () => void): () => void;
  link(target: MeshTarget): Promise<AccountMeshOutcome>;
  renameSelf(expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome>;
  revoke(target: MeshTarget): Promise<AccountMeshOutcome>;
  unpair(instanceId: string): Promise<void>;
}

// Proposed facade names; the account lifecycle owner confirms the exact DTO names at integration.
const COMMANDS = {
  state: "account_mesh_state",
  link: "account_mesh_link",
  renameSelf: "account_mesh_rename_self",
  revoke: "account_mesh_revoke",
} as const;
const CHANGE_EVENTS = ["account-mesh-changed", "mesh-state-changed"] as const;

export const bridgeAccountMeshClient: AccountMeshClient = {
  snapshot: () => invoke<AccountMeshSnapshot>(COMMANDS.state),
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
  link: (target) => invoke<AccountMeshOutcome>(COMMANDS.link, { target }),
  renameSelf: (expectedMetadataRevision, displayName) =>
    invoke<AccountMeshOutcome>(COMMANDS.renameSelf, { expectedMetadataRevision, displayName }),
  revoke: (target) => invoke<AccountMeshOutcome>(COMMANDS.revoke, { target }),
  unpair: async (instanceId) => { await invoke("mesh_disconnect", { instanceId }); },
};

const AccountMeshClientContext = createContext<AccountMeshClient>(bridgeAccountMeshClient);
export const AccountMeshClientProvider = AccountMeshClientContext.Provider;
export const useAccountMeshClient = () => useContext(AccountMeshClientContext);

export function meshTarget(row: MeshTarget): MeshTarget {
  return {
    installation_id: row.installation_id,
    key_id: row.key_id,
    key_generation: row.key_generation,
    slot_revision: row.slot_revision,
  };
}
