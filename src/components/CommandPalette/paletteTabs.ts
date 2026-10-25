import type { Tab, Workspace } from "../../lib/types";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { flattenSidebarTabs } from "../Sidebar/workspaceTabs";

export interface PaletteTabTarget {
  workspaceId: string;
  workspaceName: string;
  tabId: string;
  title: string;
}

/** Every open Tab in sidebar order, so the palette can jump to any of them. */
export function paletteTabTargets(
  workspaces: readonly Pick<Workspace, "id" | "name" | "tabs">[],
  tabs: Record<string, Tab>,
): PaletteTabTarget[] {
  return workspaces.flatMap((workspace) =>
    flattenSidebarTabs(workspace.tabs, tabs).map((tab) => ({
      workspaceId: workspace.id,
      workspaceName: workspace.name,
      tabId: tab.id,
      title: displayTitleForTab(tab),
    })),
  );
}
