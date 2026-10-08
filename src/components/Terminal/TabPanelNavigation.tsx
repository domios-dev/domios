import { planSidePanelStore } from "../Canvas/planSidePanelStore";

export function TabPanelNavigation({ tabId, view }: { tabId: string; view: "documents" | "changes" }) {
  return <nav aria-label="Tab side panel views" className="mb-3 flex gap-3">
    {(["documents", "changes"] as const).map((item) => <button key={item} type="button" aria-pressed={view === item}
      onClick={() => planSidePanelStore.open({ planId: "", documentsTabId: tabId, tabView: item })}>
      {item === "documents" ? "Documents" : "Changes"}
    </button>)}
  </nav>;
}
