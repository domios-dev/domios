import type { WorkspaceClosePreview } from "../../../gen/WorkspaceClosePreview";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import type { TransportClient } from "../../lib/transportClient";
import { assertWebRequestContext, webRequestContext } from "../../lib/webApi";
import type { TeamCloseResource } from "./teamCloseResources";

export async function readTeamClosePlanMetadata(
  workspaceId: string,
  teamId: string | null,
  signal?: AbortSignal,
  transport: Pick<TransportClient, "localJson" | "currentToken"> = baseDaemonTransport(),
): Promise<{ agentTitles: string[]; blueprints: string[]; teams: TeamCloseResource[] }> {
  const context = webRequestContext();
  const token = transport.currentToken?.();
  const query = new URLSearchParams({ resources_only: "true" });
  if (teamId !== null) query.set("team_id", teamId);
  const preview = await transport.localJson<WorkspaceClosePreview>(
    "GET", `/api/workspaces/${encodeURIComponent(workspaceId)}/close-preview?${query}`, undefined, { signal },
  );
  assertWebRequestContext(context);
  if (token !== transport.currentToken?.()) throw new DOMException("Credentials changed", "AbortError");
  if (preview.workspace_id !== workspaceId ||
    (teamId !== null && (preview.teams.length !== 1 || preview.teams[0].id !== teamId || preview.teams[0].remote))) {
    throw new Error("The close preview does not match the selected scope.");
  }
  const teams = preview.teams.map(team => ({
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
    lanes: [...new Set(team.tasks.map(task => `${task.plan_title}: ${task.task_name}`))]
      .sort((left, right) => left.localeCompare(right)),
  }));
  const teamMemberIds = new Set(teams.flatMap(team => team.members.map(member => member.tabId)));
  return {
    agentTitles: preview.agent_tabs.filter(tab => !teamMemberIds.has(tab.tab_id)).map(tab => tab.title),
    blueprints: preview.blueprints,
    teams,
  };
}
