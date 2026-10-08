import { useState, type RefObject } from "react";
import { FileText, Maximize2, FolderOpen } from "lucide-react";
import { LocalArtifactBlock, RichMarkdown, openLocalArtifact } from "@codeterm/chat-ui";
import { SidePanelFrame } from "../Canvas/planView/SidePanelFrame";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import type { PresenceState } from "../Canvas/usePresence";
import { useChatDocuments } from "./chatDocumentsStore";
import { TabPanelNavigation } from "./TabPanelNavigation";

export function DocumentsButton({ tabId }: { tabId: string }) {
  const documents = useChatDocuments(tabId);
  const entry = usePlanSidePanelEntry();
  if (!documents.length) return null;
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView !== "changes";
  return <button type="button" aria-label={`Documents (${documents.length})`} aria-expanded={open} title="Documents" className="inline-flex items-center gap-1 rounded px-1.5 text-xs text-[var(--ct-muted-fg)]" onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId })}><FileText size={14} aria-hidden />{documents.length}</button>;
}

export function DocumentsPanel({ tabId, panelRef, state, onClose }: { tabId: string; panelRef: RefObject<HTMLElement | null>; state: PresenceState; onClose: () => void }) {
  const documents = useChatDocuments(tabId);
  const [fullscreenRequest, setFullscreenRequest] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const active = documents.find((item) => item.path === selected) ?? documents[0];
  return <SidePanelFrame panelRef={panelRef} state={state} label="Documents" onClose={onClose} contentKey={tabId} title={<h2 className="plan-side-panel__title">Documents</h2>}>
    <TabPanelNavigation tabId={tabId} view="documents" />
    {!documents.length && <p>No documents recorded. Ask the agent to show a document in this tab.</p>}
    <nav aria-label="Tab documents" className="mb-3 flex flex-col gap-2">
      {documents.map((item) => <div key={item.path} className="flex items-center gap-1 rounded border border-[var(--ct-border)] p-2 text-xs">
        <button type="button" aria-pressed={active?.path === item.path} onClick={() => { setSelected(item.path); setFullscreenRequest(0); }} className="min-w-0 flex-1 text-left">
          <span className="block font-medium">{item.name}</span><span className="block truncate" title={item.path}>{item.path}</span>
          {item.shownAt != null && <time className="opacity-60">{new Date(item.shownAt).toLocaleString()}</time>}
        </button>
        <button type="button" aria-label={`Fullscreen ${item.name}`} onClick={() => { setSelected(item.path); setFullscreenRequest((value) => value + 1); }}><Maximize2 size={14} aria-hidden /></button>
        <button type="button" aria-label={`Reveal ${item.name}`} onClick={() => void openLocalArtifact(item.path).catch((failure: unknown) => setError(String(failure)))}><FolderOpen size={14} aria-hidden /></button>
      </div>)}
    </nav>
    {error && <p role="alert">{error}</p>}
    {active && <LocalArtifactBlock key={active.path} directive={active} fullscreenRequest={fullscreenRequest} fullscreenHostSelector={`[data-tab-id="${tabId}"]`} renderMarkdown={(text) => <RichMarkdown text={text} />} />}
  </SidePanelFrame>;
}
