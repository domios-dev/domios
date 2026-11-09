import { createContext, useContext } from "react";
import { invoke, listen } from "../../lib/bridge";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";

/** Exact registry slot identity the server checks before revoke or link. */
export type MeshTarget = import("../../../gen/MeshTarget").MeshTarget;

export type MeshEndpointObservation = import("../../../gen/MeshEndpointObservation").MeshEndpointObservation;

/** One account registry row as the server lists it; `endpoint` is published metadata, never reachability. */
export type AccountMachineRow = import("../../../gen/MeshManagementRow").MeshManagementRow;

export type SelfInstallation = import("../../../gen/SelfInstallation").SelfInstallation;

/** A mutually authenticated connection to an account installation. */
export type LocalPairFact = Omit<import("../../../gen/AccountMeshPair").AccountMeshPair, "installation_id" | "admission"> & {installation_id: string; admission: "account"};

export type AccountMeshFailure = import("../../../gen/AccountMeshRefusal").AccountMeshRefusal | {reason: "peer-unreachable" | "auth-refused" | "peer-needs-update" | "handshake-failed"};

export type AccountMeshOutcome = (Omit<Extract<import("../../../gen/AccountMeshOutcome").AccountMeshOutcome<{display_name: string; metadata_revision: number}>, {result: unknown}>, "ok" | "result"> & {ok: true; result?: {display_name: string; metadata_revision: number}}) | (Omit<Extract<import("../../../gen/AccountMeshOutcome").AccountMeshOutcome<unknown>, {failure: unknown}>, "ok" | "failure" | "request_id"> & {ok: false; failure: AccountMeshFailure; request_id?: string | null});

/** A genuine free machine-slot refusal from automatic enrollment, held until acknowledged. */
export type AccountMeshLimitNotice = Omit<import("../../../gen/AccountMeshLimitNotice").AccountMeshLimitNotice, "refusal"> & {refusal: unknown};

export type AccountMeshSnapshot = Omit<import("../../../gen/AccountMeshView").AccountMeshView, "registry" | "registry_failure" | "last_refusal" | "limit_notice" | "publication" | "publication_failure" | "retry_at" | "membership_expires_at"> & Partial<Pick<import("../../../gen/AccountMeshView").AccountMeshView, "publication" | "retry_at" | "membership_expires_at">> & {local_installation_id: string; registry: (Omit<NonNullable<import("../../../gen/AccountMeshView").AccountMeshView["registry"]>, "rows" | "cursor" | "registry_revision"> & Partial<Pick<NonNullable<import("../../../gen/AccountMeshView").AccountMeshView["registry"]>, "cursor" | "registry_revision">> & {rows: AccountMachineRow[]}) | null; registry_failure: AccountMeshFailure | null; last_refusal: AccountMeshFailure | null; publication_failure?: AccountMeshFailure | null; limit_notice?: AccountMeshLimitNotice | null; pairs: LocalPairFact[]; connections?: import("./connectionPhase").PeerConnection[]};

export interface AccountMeshClient {
  snapshot(): Promise<AccountMeshSnapshot>;
  enroll(): Promise<AccountMeshOutcome>;
  list(): Promise<AccountMeshOutcome>;
  subscribe(onChange: () => void): () => void;
  renameSelf(expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome>;
  renamePeer?(instanceId: string, expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome>;
  revokeSession(installationId?: string): Promise<AccountMeshOutcome>;
  revoke(target: MeshTarget): Promise<AccountMeshOutcome>;
  acknowledgeLimit(requestId: string): Promise<void>;
}

const COMMANDS = {
  state: "account_mesh_state",
  enroll: "account_mesh_enroll",
  list: "account_mesh_list",
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
  renameSelf: (expectedMetadataRevision, displayName) =>
    invoke<AccountMeshOutcome>(COMMANDS.renameSelf, { expectedMetadataRevision, displayName }),
  renamePeer: (instanceId, expectedMetadataRevision, displayName) => daemonTransportForPeer(instanceId).json("POST", "/api/account/mesh/rename", {
    expected_metadata_revision: expectedMetadataRevision, display_name: displayName,
  }),
  revokeSession: (installationId) => invoke<AccountMeshOutcome>("account_mesh_session_revoke", { installationId }),
  revoke: (target) => invoke<AccountMeshOutcome>(COMMANDS.revoke, { target }),
  acknowledgeLimit: async (requestId) => { await invoke(COMMANDS.acknowledgeLimit, { requestId }); },
};

const AccountMeshClientContext = createContext<AccountMeshClient>(bridgeAccountMeshClient);
export const AccountMeshClientProvider = AccountMeshClientContext.Provider;
export const useAccountMeshClient = () => useContext(AccountMeshClientContext);

