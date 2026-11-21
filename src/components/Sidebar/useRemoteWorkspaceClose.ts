import { useCallback, useEffect, useRef, useState } from "react";
import type { WorkspaceClosePreview } from "../../../gen/WorkspaceClosePreview";
import { daemonTransportForPeer, onDaemonTransport } from "../../lib/daemonBootstrap";
import { createTabsTransport } from "../../lib/transport/tabs";
import type { TransportClient } from "../../lib/transportClient";
import { WebApiError, webRequestContext } from "../../lib/webApi";
import type { TeamCloseResource } from "./teamCloseResources";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";

export interface RemoteWorkspaceCloseTarget {
  peerId: string;
  workspaceId: string;
  workspaceName: string;
  machineName: string;
  availability: GeneralAvailability;
  protected: boolean;
}

export interface RemoteWorkspaceCloseConfirmation extends RemoteWorkspaceCloseTarget {
  agentTitles: string[];
  crons: string[];
  blueprints: string[];
  teams: TeamCloseResource[];
  warnings?: string[];
}

type RemoteWorkspaceCloseInventory = Pick<RemoteWorkspaceCloseConfirmation,
  "workspaceName" | "agentTitles" | "crons" | "blueprints" | "teams" | "warnings">;

export interface RemoteWorkspaceClosePort {
  readInventory(peerId: string, workspaceId: string, signal: AbortSignal): Promise<RemoteWorkspaceCloseInventory>;
  closeWorkspace(peerId: string, workspaceId: string): Promise<void>;
  workspaceExists(peerId: string, workspaceId: string): Promise<boolean>;
}

export type RemoteWorkspaceCloseRequestResult =
  | { kind: "ready" }
  | { kind: "offline" }
  | { kind: "protected" }
  | { kind: "stale" }
  | { kind: "inventory_failed"; detail: string };

export type RemoteWorkspaceCloseResult =
  | { kind: "closed"; target: RemoteWorkspaceCloseConfirmation }
  | { kind: "closed_with_warning"; target: RemoteWorkspaceCloseConfirmation; detail: string }
  | { kind: "still_present"; target: RemoteWorkspaceCloseConfirmation; detail: string }
  | { kind: "unknown"; target: RemoteWorkspaceCloseConfirmation; detail: string }
  | { kind: "offline"; target: RemoteWorkspaceCloseConfirmation }
  | { kind: "none" };

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isStaleCredentialError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

export const remoteWorkspaceClosePort: RemoteWorkspaceClosePort = {
  async readInventory(peerId, workspaceId, signal) {
    const transport = daemonTransportForPeer(peerId);
    const tokenBefore = transport.currentToken?.() ?? null;
    const contextBefore = webRequestContext();
    const preview = await transport.json<WorkspaceClosePreview>(
      "GET",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/close-preview`,
      undefined,
      { signal },
    );
    if (webRequestContext() !== contextBefore || (transport.currentToken?.() ?? null) !== tokenBefore) {
      throw new DOMException("Credentials changed", "AbortError");
    }
    if (preview.installation_id !== peerId || preview.workspace_id !== workspaceId) {
      throw new Error("The close preview does not match the selected machine and workspace.");
    }
    if (preview.protected) {
      throw new Error("The selected workspace is protected and cannot be closed.");
    }
    const teamMemberIds = new Set(preview.teams.flatMap(team => team.members.map(member => member.tab_id)));
    const teams: TeamCloseResource[] = preview.teams.map(team => ({
      id: team.id,
      name: team.name,
      members: team.members.map(member => ({
        tabId: member.tab_id,
        title: member.title,
        role: member.role,
        roleProfile: member.role_profile,
      })),
      crons: team.crons,
      blueprints: team.blueprints,
      lanes: [
        ...team.tasks.map(task => `${task.plan_title}: ${task.task_name}${task.orphan ? " (orphan task)" : ""}${task.remote_attempt ? " (remote attempt)" : ""}`),
        ...team.open_attempts.map(attempt => `Open attempt ${attempt.attempt_id}${attempt.executor_tab_id ? ` · ${attempt.executor_tab_id}` : ""}${attempt.remote ? " (remote)" : ""}`),
        ...team.lanes.map(lane => [lane.path, lane.branch, lane.state, lane.detail].filter(Boolean).join(" · ")),
      ],
    }));
    const agentTitles = preview.agent_tabs
      .filter(tab => !teamMemberIds.has(tab.tab_id))
      .map(tab => tab.title);
    const warnings = preview.warnings.map(warning => warning.detail);
    return {
      workspaceName: preview.workspace_name,
      agentTitles,
      crons: preview.crons,
      blueprints: preview.blueprints,
      teams,
      warnings,
    };
  },
  async closeWorkspace(peerId, workspaceId) {
    const transport = createTabsTransport(daemonTransportForPeer(peerId));
    await transport.mux_close_workspace({ workspaceId, force: true });
  },
  async workspaceExists(peerId, workspaceId) {
    try {
      const preview = await daemonTransportForPeer(peerId).json<WorkspaceClosePreview>(
        "GET",
        `/api/workspaces/${encodeURIComponent(workspaceId)}/close-preview`,
      );
      if (preview.installation_id !== peerId || preview.workspace_id !== workspaceId) {
        throw new Error("The close preview does not match the selected machine and workspace.");
      }
      return true;
    } catch (error) {
      if (error instanceof WebApiError && error.status === 404) return false;
      if (error instanceof WebApiError && (error.status === 409 || error.status === 413)) return true;
      throw error;
    }
  },
};

export function useRemoteWorkspaceClose(
  availabilityForPeer: (peerId: string) => GeneralAvailability | undefined,
  onClosed: (peerId: string, workspaceId: string) => void,
  port: RemoteWorkspaceClosePort = remoteWorkspaceClosePort,
) {
  const [pending, setPending] = useState<RemoteWorkspaceCloseConfirmation | null>(null);
  const pendingRef = useRef<RemoteWorkspaceCloseConfirmation | null>(null);
  const generation = useRef(0);
  const inventoryRead = useRef<AbortController | null>(null);

  useEffect(() => {
    if (typeof onDaemonTransport !== "function") return;
    let initialized = false;
    let previous: TransportClient | null = null;
    return onDaemonTransport((client) => {
      if (!initialized) {
        initialized = true;
        previous = client;
        return;
      }
      if (client === previous) return;
      previous = client;
      generation.current++;
      inventoryRead.current?.abort();
      inventoryRead.current = null;
      pendingRef.current = null;
      setPending(null);
    });
  }, []);

  const request = useCallback(async (
    target: RemoteWorkspaceCloseTarget,
    isStillSelected: () => boolean,
  ): Promise<RemoteWorkspaceCloseRequestResult> => {
    if (target.availability !== "online" || availabilityForPeer(target.peerId) !== "online") return { kind: "offline" };
    if (target.protected) return { kind: "protected" };
    if (!isStillSelected()) return { kind: "stale" };
    const requestGeneration = ++generation.current;
    const contextBefore = webRequestContext();
    inventoryRead.current?.abort();
    const controller = new AbortController();
    inventoryRead.current = controller;
    pendingRef.current = null;
    setPending(null);
    const isStale = () => controller.signal.aborted
      || requestGeneration !== generation.current
      || webRequestContext() !== contextBefore
      || !isStillSelected()
      || availabilityForPeer(target.peerId) !== "online";
    try {
      const inventory = await port.readInventory(target.peerId, target.workspaceId, controller.signal);
      if (isStale()) return { kind: "stale" };
      const confirmation = { ...target, ...inventory };
      pendingRef.current = confirmation;
      setPending(confirmation);
      return { kind: "ready" };
    } catch (error) {
      if (isStale() || isStaleCredentialError(error)) return { kind: "stale" };
      return { kind: "inventory_failed", detail: errorMessage(error) };
    } finally {
      if (inventoryRead.current === controller) inventoryRead.current = null;
    }
  }, [availabilityForPeer, port]);

  const cancel = useCallback(() => {
    generation.current++;
    inventoryRead.current?.abort();
    inventoryRead.current = null;
    pendingRef.current = null;
    setPending(null);
  }, []);

  const confirm = useCallback(async (): Promise<RemoteWorkspaceCloseResult> => {
    const target = pendingRef.current;
    if (!target) return { kind: "none" };
    generation.current++;
    inventoryRead.current?.abort();
    inventoryRead.current = null;
    pendingRef.current = null;
    setPending(null);
    if (availabilityForPeer(target.peerId) !== "online") return { kind: "offline", target };
    let closeError: string | null = null;
    try {
      await port.closeWorkspace(target.peerId, target.workspaceId);
    } catch (error) {
      closeError = errorMessage(error);
    }
    let exists: boolean;
    try {
      exists = await port.workspaceExists(target.peerId, target.workspaceId);
    } catch (error) {
      return { kind: "unknown", target, detail: [closeError, `Workspace close status could not be checked: ${errorMessage(error)}`].filter(Boolean).join("\n") };
    }
    if (exists) {
      return { kind: "still_present", target, detail: closeError ?? "The peer still reports this workspace after the close request." };
    }
    onClosed(target.peerId, target.workspaceId);
    return closeError ? { kind: "closed_with_warning", target, detail: closeError } : { kind: "closed", target };
  }, [availabilityForPeer, onClosed, port]);

  return { pending, request, cancel, confirm };
}
