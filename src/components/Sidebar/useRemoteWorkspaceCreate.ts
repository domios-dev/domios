import { useCallback, useRef } from "react";
import { authoredCause } from "../../lib/describeError";
import type { Notice } from "../../lib/notices";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { createTabsTransport } from "../../lib/transport/tabs";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";
import { remotePeerActionEligibility, workspaceNameFromPath } from "./remoteSidebarActionsModel";

function peerRefusalReason(raw: string): string {
  return raw.replace(/^.*? → HTTP \d{3}: /, "").trim() || raw.trim();
}

export function remoteWorkspaceCreateErrorNotice(
  peerId: string,
  machineName: string,
  result: Extract<RemoteWorkspaceCreateResult, { kind: "unavailable" | "failed" }>,
): Notice {
  return {
    kind: "toast",
    id: `workspace-create:${peerId}`,
    tabId: null,
    workspaceId: null,
    title: result.kind === "unavailable" ? "Workspace create not sent" : "Workspace creation failed",
    detail: result.kind === "unavailable"
      ? authoredCause(`${machineName} is ${result.reason}.`)
      : JSON.stringify({
        code: "workspace_close_refused",
        message: peerRefusalReason(result.detail),
        cause: `Could not create the workspace on ${machineName}.`,
        raw: `${machineName}\n${result.detail}`,
      }),
    severity: "Error",
    persistent: true,
    retainInNotifications: true,
  };
}

export interface CreatedRemoteWorkspace {
  id: string;
  name: string;
}

export interface RemoteWorkspaceCreatePort {
  createWorkspace(peerId: string, name: string, cwd: string): Promise<CreatedRemoteWorkspace>;
}

export type RemoteWorkspaceCreateResult =
  | { kind: "created"; peerId: string; workspace: CreatedRemoteWorkspace }
  | { kind: "unavailable"; peerId: string; reason: string }
  | { kind: "failed"; peerId: string; detail: string }
  | { kind: "in_flight"; peerId: string };

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export const remoteWorkspaceCreatePort: RemoteWorkspaceCreatePort = {
  async createWorkspace(peerId, name, cwd) {
    const transport = createTabsTransport(daemonTransportForPeer(peerId));
    const result = await transport.mux_create_workspace({ name, cwd, activate: false }) as Partial<CreatedRemoteWorkspace> | null;
    if (!result || typeof result.id !== "string" || !result.id.trim()) {
      throw new Error("The selected machine did not return the created workspace.");
    }
    return { id: result.id, name: typeof result.name === "string" && result.name.trim() ? result.name : name };
  },
};

export function useRemoteWorkspaceCreate(
  availabilityForPeer: (peerId: string) => GeneralAvailability | undefined,
  onCreated: (peerId: string, workspace: CreatedRemoteWorkspace) => void,
  port: RemoteWorkspaceCreatePort = remoteWorkspaceCreatePort,
) {
  const inFlight = useRef(new Set<string>());

  const create = useCallback(async (
    peerId: string,
    cwd: string,
    explicitName?: string,
  ): Promise<RemoteWorkspaceCreateResult> => {
    const eligibility = remotePeerActionEligibility(availabilityForPeer(peerId));
    if (!eligibility.allowed) {
      return { kind: "unavailable", peerId, reason: eligibility.reason ?? "offline" };
    }
    const key = `${peerId}:${cwd}`;
    if (inFlight.current.has(key)) return { kind: "in_flight", peerId };
    inFlight.current.add(key);
    const name = explicitName?.trim() || workspaceNameFromPath(cwd);
    try {
      const workspace = await port.createWorkspace(peerId, name, cwd);
      onCreated(peerId, workspace);
      return { kind: "created", peerId, workspace };
    } catch (error) {
      return { kind: "failed", peerId, detail: errorMessage(error) };
    } finally {
      inFlight.current.delete(key);
    }
  }, [availabilityForPeer, onCreated, port]);

  return { create };
}
