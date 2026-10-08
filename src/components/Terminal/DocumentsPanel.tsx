import { useEffect, useState, type RefObject } from "react";
import { FileText, Maximize2, FolderOpen } from "lucide-react";
import { LocalArtifactBlock, RichMarkdown, openLocalArtifact } from "@codeterm/chat-ui";
import { SidePanelFrame } from "../Canvas/planView/SidePanelFrame";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import type { PresenceState } from "../Canvas/usePresence";
import { chatDocumentsStore, useChatDocumentSnapshot, useChatDocuments } from "./chatDocumentsStore";
import { DocumentsTree } from "./DocumentsTree";
import { TabPanelNavigation } from "./TabPanelNavigation";

export function DocumentsButton({ tabId }: { tabId: string }) {
  const documents = useChatDocuments(tabId);
  const entry = usePlanSidePanelEntry();
  if (!documents.length) return null;
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView !== "changes";
  return <button type="button" aria-label={`Documents (${documents.length})`} aria-expanded={open} title="Documents" className="inline-flex items-center gap-1 rounded px-1.5 text-xs text-[var(--ct-muted-fg)]" onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId })}><FileText size={14} aria-hidden />{documents.length}</button>;
}

function useRefreshDocuments(tabId: string, open: boolean) {
  useEffect(() => { if (open) void chatDocumentsStore.refresh(tabId); }, [tabId, open]);
}

export function DocumentsPanel({ tabId, panelRef, state, onClose }: { tabId: string; panelRef: RefObject<HTMLElement | null>; state: PresenceState; onClose: () => void }) {
  const { documents, home } = useChatDocumentSnapshot(tabId);
  useRefreshDocuments(tabId, state !== "closing");
  const [fullscreenRequest, setFullscreenRequest] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const active = documents.find((item) => item.path === selected) ?? documents[0];
  function select(path: string) { setSelected(path); setFullscreenRequest(0); }
  function fullscreen(path: string) { setSelected(path); setFullscreenRequest((value) => value + 1); }
  return <SidePanelFrame panelRef={panelRef} state={state} label="Documents" onClose={onClose} contentKey={tabId} title={<h2 className="plan-side-panel__title">Documents</h2>}>
    <TabPanelNavigation tabId={tabId} view="documents" />
    {!documents.length && <p>No documents available. Ask the agent to show a document in this tab.</p>}
    <DocumentsTree key={tabId} documents={documents} home={home} selected={active?.path} onSelect={select} onFullscreen={fullscreen} renderActions={(item) => <>
      <button type="button" aria-label={`Fullscreen ${item.name}`} title="Fullscreen" onClick={() => fullscreen(item.path)}><Maximize2 size={14} aria-hidden /></button>
      <button type="button" aria-label={`Reveal ${item.name}`} title="Reveal in file manager" onClick={() => void openLocalArtifact(item.path).catch((failure: unknown) => setError(String(failure)))}><FolderOpen size={14} aria-hidden /></button>
    </>} />
    {error && <p role="alert">{error}</p>}
    {active && <LocalArtifactBlock key={active.path} directive={active} fullscreenRequest={fullscreenRequest} fullscreenHostSelector={`[data-tab-id="${tabId}"]`} renderMarkdown={(text) => <RichMarkdown text={text} />} />}
  </SidePanelFrame>;
}
