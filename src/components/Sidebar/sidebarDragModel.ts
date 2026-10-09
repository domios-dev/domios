import type { Tab } from "../../lib/types";
import { effectiveTeamId, groupWorkspaceTeamMembers, partitionTeamMembers } from "./workspaceTeamModel";

/** One draggable top-level unit of a workspace: a loose tab or a whole team block, with every row it renders. */
export interface DragUnit {
  key: string;
  kind: "tab" | "team";
  tabIds: string[];
}

export interface SidebarDragModel {
  units: DragUnit[];
  /** A row followed by every row rendered under it (attachments, nested children). */
  blockOf: (tabId: string) => readonly string[];
  attachedIds: ReadonlySet<string>;
  looseTabs: Tab[];
  looseChildIds: ReadonlySet<string>;
}

/** Units of the visible workspace tabs, each expanded into its rows in display order. */
export function buildSidebarDragModel(
  visibleTabs: readonly Tab[],
  attachmentsByOwner: ReadonlyMap<string, readonly Tab[]>,
): SidebarDragModel {
  const attachedIds = new Set([...attachmentsByOwner.values()].flat().map((tab) => tab.id));
  const looseTabs = visibleTabs.filter((tab) => !effectiveTeamId(tab) && !attachedIds.has(tab.id));
  const looseIds = new Set(looseTabs.map((tab) => tab.id));
  const looseChildIds = new Set(
    looseTabs.filter((tab) => tab.parent_tab_id && looseIds.has(tab.parent_tab_id)).map((tab) => tab.id),
  );
  const blocks = new Map<string, string[]>();
  const withAttachments = (tabId: string) => [tabId, ...(attachmentsByOwner.get(tabId) ?? []).map((tab) => tab.id)];
  const looseBlock = (tab: Tab, visiting: Set<string>): string[] => {
    const cached = blocks.get(tab.id);
    if (cached) return cached;
    visiting.add(tab.id);
    const children = looseTabs.filter((child) => child.parent_tab_id === tab.id && !visiting.has(child.id));
    const block = [...withAttachments(tab.id), ...children.flatMap((child) => looseBlock(child, visiting))];
    blocks.set(tab.id, block);
    return block;
  };

  const membersByTeam = groupWorkspaceTeamMembers(visibleTabs);
  const units: DragUnit[] = [];
  const seenTeams = new Set<string>();
  for (const tab of visibleTabs) {
    if (attachedIds.has(tab.id)) continue;
    const teamId = effectiveTeamId(tab);
    if (!teamId) {
      if (looseChildIds.has(tab.id)) continue;
      units.push({ key: tab.id, kind: "tab", tabIds: looseBlock(tab, new Set()) });
      continue;
    }
    if (seenTeams.has(teamId)) continue;
    seenTeams.add(teamId);
    const members = (membersByTeam.get(teamId) ?? []).filter((member) => !attachedIds.has(member.tab.id));
    const { managers, workers, terminals } = partitionTeamMembers(members);
    const memberIds = [...managers, ...workers, ...terminals].map((member) => member.tab.id);
    for (const id of memberIds) blocks.set(id, withAttachments(id));
    units.push({ key: teamId, kind: "team", tabIds: memberIds.flatMap((id) => blocks.get(id) ?? [id]) });
  }
  return {
    units,
    blockOf: (tabId) => blocks.get(tabId) ?? [tabId],
    attachedIds,
    looseTabs,
    looseChildIds,
  };
}

/**
 * `sort_order` is one workspace-wide sequence across every tab level, so a drag is
 * expanded into the full display order: units in their new order, then any workspace
 * tab no unit renders (hidden), keeping its relative place.
 */
export function workspaceTabOrder(workspaceTabIds: readonly string[], units: readonly DragUnit[]): string[] {
  const known = new Set(workspaceTabIds);
  const emitted = new Set<string>();
  const order: string[] = [];
  for (const unit of units) {
    for (const id of unit.tabIds) {
      if (!known.has(id) || emitted.has(id)) continue;
      emitted.add(id);
      order.push(id);
    }
  }
  for (const id of workspaceTabIds) if (!emitted.has(id)) order.push(id);
  return order;
}

/** Units reordered to `orderedKeys`; keys missing from it keep their trailing order. */
export function unitsInOrder(units: readonly DragUnit[], orderedKeys: readonly string[]): DragUnit[] {
  const byKey = new Map(units.map((unit) => [unit.key, unit]));
  const ordered = orderedKeys.map((key) => byKey.get(key)).filter((unit): unit is DragUnit => unit != null);
  const seen = new Set(ordered.map((unit) => unit.key));
  return [...ordered, ...units.filter((unit) => !seen.has(unit.key))];
}

/** The unit holding one sibling group (team section, children, attachments) with that group reordered in its slot. */
export function withSiblingOrder(
  units: readonly DragUnit[],
  orderedSiblings: readonly string[],
  blockOf: (tabId: string) => readonly string[],
): DragUnit[] {
  const siblings = new Set(orderedSiblings);
  return units.map((unit) => {
    const slot = unit.tabIds.findIndex((id) => siblings.has(id));
    if (slot < 0) return unit;
    const moved = orderedSiblings.flatMap((id) => [...blockOf(id)]);
    const movedIds = new Set(moved);
    const keep = (id: string) => !movedIds.has(id);
    return {
      ...unit,
      tabIds: [...unit.tabIds.slice(0, slot).filter(keep), ...moved, ...unit.tabIds.slice(slot).filter(keep)],
    };
  });
}
