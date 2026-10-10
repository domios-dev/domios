import { invoke } from "../../lib/bridge";
import { ORCHESTRATOR_KIND, roleKind } from "../../lib/roleKind";
import { requestTeamMember, selectTeamStaffingManager, type TeamMemberAttachment, type TeamMemberRequest } from "../../lib/teamActions";
import type { MultiplexerState, OrchestrationPairInput, OrchestrationPairResult } from "../../lib/types";
import { buildCartographerMapTask, selectCartographerMapTeam, type CartographerMapDraft } from "./cartographerMapModel";

export interface CartographerMapRequest {
  workspaceId: string;
  projectPaths: readonly string[];
  draft: CartographerMapDraft;
}

interface CartographerMapActions {
  snapshot: () => Promise<MultiplexerState>;
  createPair: (input: OrchestrationPairInput) => Promise<OrchestrationPairResult>;
  addMember: (request: TeamMemberRequest) => Promise<TeamMemberAttachment>;
}

export function cartographerMapActions(
  createPair: CartographerMapActions["createPair"],
): CartographerMapActions {
  return {
    snapshot: () => invoke<MultiplexerState>("mux_get_state"),
    createPair,
    addMember: async (request) => await requestTeamMember(invoke, request) as TeamMemberAttachment,
  };
}

export async function startCartographerMap(
  request: CartographerMapRequest,
  actions: CartographerMapActions,
): Promise<TeamMemberAttachment> {
  if (!request.draft.providerId) throw new Error("Choose a provider for the Cartographer");
  let snapshot = await actions.snapshot();
  let workspace = snapshot.workspaces.find((candidate) => candidate.id === request.workspaceId);
  if (!workspace) throw new Error("The map workspace is no longer open");
  let team = selectCartographerMapTeam(workspace, snapshot.teams ?? [], snapshot.tabs);
  let managerTabId: string | null = null;
  if (!team) {
    const pair = await actions.createPair({
      workspaceId: workspace.id,
      orchestratorAgent: request.draft.providerId,
      orchestratorModel: request.draft.modelId,
      plannerAgent: request.draft.providerId,
      plannerModel: request.draft.modelId,
    });
    snapshot = await actions.snapshot();
    workspace = snapshot.workspaces.find((candidate) => candidate.id === request.workspaceId);
    const anchor = snapshot.tabs[pair.orchestrator_tab_id];
    team = snapshot.teams?.find((candidate) => candidate.id === anchor?.team_id && candidate.workspace_id === request.workspaceId) ?? null;
    if (!workspace || !team || !anchor || !workspace.tabs.includes(anchor.id)) {
      throw new Error("The created Team is not attached to the map workspace");
    }
    managerTabId = anchor.id;
  } else {
    const members = workspace.tabs.flatMap((tabId) => {
      const tab = snapshot.tabs[tabId];
      return tab && tab.team_id === team?.id && tab.agent_type && !tab.remote_peer_id && tab.boot_stage !== "spawn_failed" ? [tab] : [];
    });
    managerTabId = selectTeamStaffingManager(members);
    if (!managerTabId) {
      if (members.some((tab) => roleKind(tab) === ORCHESTRATOR_KIND)) {
        throw new Error("This Team's Orchestrator does not allow adding members");
      }
      const anchor = await actions.addMember({
        teamId: team.id, role: "orchestrator", providerId: request.draft.providerId,
        managerTabId: null, model: request.draft.modelId,
      });
      if (anchor.team.id !== team.id || anchor.team.workspace_id !== workspace.id) {
        throw new Error("The replacement Orchestrator belongs to a different Team");
      }
      managerTabId = anchor.tab_id;
    }
  }
  const attachment = await actions.addMember({
    teamId: team.id,
    role: "cartographer",
    providerId: request.draft.providerId,
    managerTabId,
    model: request.draft.modelId,
    brief: buildCartographerMapTask(request.draft, request.projectPaths),
  });
  if (!attachment.tab_id || attachment.team.id !== team.id || attachment.team.workspace_id !== workspace.id) {
    throw new Error("The Cartographer is not attached to the requested workspace Team");
  }
  return attachment;
}
