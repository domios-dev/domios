import { useCallback, useRef, useState } from "react";
import type { PlanView } from "../Canvas/planProjection";
import type { BlueprintSnapshot } from "../Automation/types";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { createOrchestrationTransport } from "../../lib/transport/orchestration";
import { createTabsTransport } from "../../lib/transport/tabs";
import type { MultiplexerState, Workspace } from "../../lib/types";
import { teamCloseResourcesForWorkspace, type TeamCloseResource } from "./teamCloseResources";
import { flattenSidebarTabs, tabsInWorkspace } from "./workspaceTabs";
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
  blueprints: string[];
  teams: TeamCloseResource[];
}

export interface RemoteWorkspaceClosePort {
  readInventory(peerId: string, workspaceId: string, signal: AbortSignal): Promise<Pick<RemoteWorkspaceCloseConfirmation, "workspaceName" | "agentTitles" | "blueprints" | "teams">>;
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

async function readPlanViews(peerId: string, signal: AbortSignal) {
  const client = createOrchestrationTransport(daemonTransportForPeer(peerId));
  const index = await client.list_plan_index({ signal });
  // Close inventory also depends on lane and open-attempt owners, which the index omits.
  const plans: PlanView[] = [];
  for (let offset = 0; offset < index.length; offset += 4) {
    signal.throwIfAborted();
    const batch = await Promise.all(index.slice(offset, offset + 4).map(({ plan_id }) => client.get_plan_full({ planId: plan_id }, { signal })));
    plans.push(...batch as unknown as PlanView[]);
  }
  return plans;
}

const remoteWorkspaceClosePort: RemoteWorkspaceClosePort = {
  async readInventory(peerId, workspaceId, signal) {
    const transport = daemonTransportForPeer(peerId);
    const orchestration = createOrchestrationTransport(transport);
    const [snapshot, crons, graph] = await Promise.all([
      transport.json<MultiplexerState>("GET", "/api/mux/state", undefined, { signal }),
      orchestration.cron_list({ workspaceId }),
      orchestration.automation_graph_workspace_snapshot({ workspaceId }, { signal }),
    ]);
    const workspace = snapshot.workspaces.find(candidate => candidate.id === workspaceId);
    if (!workspace || workspace.protected) throw new Error("The selected workspace is unavailable or protected on that machine.");
    const workspaceTabs = tabsInWorkspace(workspace.tabs, snapshot.tabs);
    const plans = await readPlanViews(peerId, signal);
    const blueprints: readonly BlueprintSnapshot[] = graph.blueprints ?? [];
    const teams = teamCloseResourcesForWorkspace(workspace, snapshot.tabs, snapshot.teams ?? [], crons, plans, blueprints);
    const teamMemberIds = new Set(teams.flatMap(team => team.members.map(member => member.tabId)));
    const agentTitles = flattenSidebarTabs(workspace.tabs, snapshot.tabs)
      .filter(tab => tab.agent_type && !teamMemberIds.has(tab.id)).map(tab => tab.title);
    const workspaceTabIds = new Set(workspaceTabs.map(tab => tab.id));
    const tabBlueprints = blueprints.filter(blueprint => blueprint.state === "running" && blueprint.owner?.kind === "tab"
      && blueprint.owner.tab_id !== null && workspaceTabIds.has(blueprint.owner.tab_id) && !teamMemberIds.has(blueprint.owner.tab_id))
      .map(blueprint => blueprint.name ?? blueprint.id);
    return { workspaceName: workspace.name, agentTitles, blueprints: tabBlueprints, teams };
  },
  async closeWorkspace(peerId, workspaceId) {
    const transport = createTabsTransport(daemonTransportForPeer(peerId));
    await transport.mux_close_workspace({ workspaceId, force: true });
  },
  async workspaceExists(peerId, workspaceId) {
    const snapshot = await daemonTransportForPeer(peerId).json<MultiplexerState>("GET", "/api/mux/state");
    return snapshot.workspaces.some((workspace: Workspace) => workspace.id === workspaceId);
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

  const request = useCallback(async (
    target: RemoteWorkspaceCloseTarget,
    isStillSelected: () => boolean,
  ): Promise<RemoteWorkspaceCloseRequestResult> => {
    if (target.availability !== "online" || availabilityForPeer(target.peerId) !== "online") return { kind: "offline" };
    if (target.protected) return { kind: "protected" };
    if (!isStillSelected()) return { kind: "stale" };
    const requestGeneration = ++generation.current;
    inventoryRead.current?.abort();
    const controller = new AbortController();
    inventoryRead.current = controller;
    pendingRef.current = null;
    setPending(null);
    try {
      const inventory = await port.readInventory(target.peerId, target.workspaceId, controller.signal);
      if (controller.signal.aborted || requestGeneration !== generation.current || !isStillSelected()) return { kind: "stale" };
      const confirmation = { ...target, ...inventory };
      pendingRef.current = confirmation;
      setPending(confirmation);
      return { kind: "ready" };
    } catch (error) {
      if (controller.signal.aborted || requestGeneration !== generation.current || !isStillSelected()) return { kind: "stale" };
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
