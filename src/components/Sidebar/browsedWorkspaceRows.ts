import type { ObservedPeerTab } from "../../store/multiplexer/pairedWorkspaceProjection";
import { isConciergeTab } from "../../lib/tabInputPolicy";
import type { Tab } from "../../lib/types";
import { buildSidebarDragModel } from "./sidebarDragModel";
import { surfaceAttachmentsByOwner } from "./surfaceAttachmentModel";
import { groupWorkspaceTeamMembers } from "./workspaceTeamModel";

export function browsedWorkspaceRows(tabs: readonly ObservedPeerTab[]) {
  const visible = tabs.filter(({ tab }) => !isConciergeTab(tab));
  const viewerIds = new Map(visible.map(({ tab, viewerId }) => [tab.id, viewerId]));
  const byViewer = new Map(visible.map(session => [session.viewerId, session]));
  const rows = visible.map(({ tab, viewerId }) => ({
    ...tab, id: viewerId, parent_tab_id: tab.parent_tab_id ? viewerIds.get(tab.parent_tab_id) : undefined,
  }));
  const rowsById: Record<string, Tab> = Object.fromEntries(rows.map(tab => [tab.id, tab]));
  const attachmentsByOwner = surfaceAttachmentsByOwner(rows);
  const drag = buildSidebarDragModel(rows, attachmentsByOwner);
  const membersByTeam = groupWorkspaceTeamMembers(rows);
  return { byViewer, rowsById, attachmentsByOwner, drag, membersByTeam };
}
