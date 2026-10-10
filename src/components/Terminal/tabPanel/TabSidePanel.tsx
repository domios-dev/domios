import type { RefObject } from "react";
import { formatCompact, formatExact } from "../../../lib/formatCompact";
import { planSidePanelStore } from "../../Canvas/planSidePanelStore";
import { SidePanelFrame } from "../../Canvas/planView/SidePanelFrame";
import type { PresenceState } from "../../Canvas/usePresence";
import { Segmented } from "../../shared/Segmented";
import { useChatChanges } from "../chatChangesStore";
import { useChatDocuments } from "../chatDocumentsStore";
import { ChangesView } from "../ChangesPanel";
import { DocumentsView } from "../DocumentsPanel";
import type { TabPanelView } from "./tabPanelNavigation";

const VIEW_LABEL: Record<TabPanelView, string> = { documents: "Documents", changes: "Changes" };

function TabPanelSwitch({ tabId, view }: { tabId: string; view: TabPanelView }) {
  const counts: Record<TabPanelView, number> = { documents: useChatDocuments(tabId).length, changes: useChatChanges(tabId).cumulative.length };
  return <div className="tab-panel__switch">
    <Segmented aria-label="Side panel view" value={view} height={28}
      options={(["documents", "changes"] as const).map((value) => ({
        value, label: VIEW_LABEL[value], title: `${VIEW_LABEL[value]} (${formatExact(counts[value])})`,
        count: counts[value] ? formatCompact(counts[value]) : undefined,
      }))}
      onChange={(next) => planSidePanelStore.open({ planId: "", documentsTabId: tabId, tabView: next })} />
  </div>;
}

/** One frame for a tab's Documents and Changes: the switch stays mounted while the view below it changes. */
export function TabSidePanel({ tabId, view, cwd, panelRef, state, onClose }: {
  tabId: string; view: TabPanelView; cwd?: string | null;
  panelRef: RefObject<HTMLElement | null>; state: PresenceState; onClose: () => void;
}) {
  return <SidePanelFrame panelRef={panelRef} state={state} label={VIEW_LABEL[view]} closeLabel="Close side panel" onClose={onClose} contentKey={tabId}
    data-testid="tab-side-panel" data-view={view} title={<TabPanelSwitch tabId={tabId} view={view} />}>
    {view === "changes" ? <ChangesView tabId={tabId} cwd={cwd} /> : <DocumentsView tabId={tabId} open={state !== "closing"} />}
  </SidePanelFrame>;
}
