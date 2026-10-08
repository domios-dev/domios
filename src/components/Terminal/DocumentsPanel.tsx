import { useEffect, useRef, useState } from "react";
import { FileCode, FileText, FolderOpen, Image } from "lucide-react";
import { openLocalArtifact } from "@codeterm/chat-ui";
import { formatCompact } from "../../lib/formatCompact";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import type { ChatDocument } from "./chatDocuments";
import { chatDocumentsStore, useChatDocumentSnapshot, useChatDocuments } from "./chatDocumentsStore";
import { insertIntoComposer } from "./composerInsertion";
import { DocumentDetail } from "./tabPanel/DocumentDetail";
import { ChatAction, IconAction, PanelEmpty, PanelPage } from "./tabPanel/PanelParts";
import { PanelFileTree } from "./tabPanel/PanelFileTree";
import { buildPathTree, shortenPath } from "./tabPanel/pathTree";
import { resolveDetail, useTabPanelNavigation } from "./tabPanel/tabPanelNavigation";

export function DocumentsButton({ tabId }: { tabId: string }) {
  const documents = useChatDocuments(tabId);
  const entry = usePlanSidePanelEntry();
  if (!documents.length) return null;
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView !== "changes";
  return <button type="button" aria-label={`Documents (${documents.length})`} aria-expanded={open} title={`Documents (${documents.length})`}
    className="inline-flex items-center gap-1 rounded px-1.5 text-xs tabular-nums text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
    onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId })}>
    <FileText size={14} aria-hidden />{formatCompact(documents.length)}
  </button>;
}

const DOCUMENT_ICON = { image: Image, page: FileCode, document: FileText } as const;

function documentTitle(document: ChatDocument): string {
  return `${document.path}${document.shownAt == null ? "" : `\nLast shown: ${new Date(document.shownAt).toLocaleString()}`}`;
}

export function DocumentsView({ tabId, open }: { tabId: string; open: boolean }) {
  const { documents, home } = useChatDocumentSnapshot(tabId);
  useEffect(() => { if (open) void chatDocumentsStore.refresh(tabId); }, [tabId, open]);
  const [navigation, dispatch] = useTabPanelNavigation(tabId);
  const returnTo = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const detail = resolveDetail(navigation.documents.detail, documents.map((document) => document.path));
  const active = documents.find((document) => document.path === detail);
  const back = () => { returnTo.current = detail; dispatch({ type: "back", view: "documents" }); };
  if (active) return <DocumentDetail key={active.path} tabId={tabId} document={active} home={home} onBack={back} />;
  if (!documents.length) return <PanelPage><PanelEmpty icon={FileText}>Documents the agent shows in this tab appear here.</PanelEmpty></PanelPage>;
  const nodes = buildPathTree(documents.map((document) => ({ id: document.path, path: shortenPath(document.path, home), item: document })));
  return <PanelPage>
    {error && <p role="alert" className="tab-panel__note">{error}</p>}
    <PanelFileTree label="Documents" nodes={nodes} current={returnTo.current} onOpen={(node) => dispatch({ type: "open", view: "documents", id: node.id })}
      leaf={({ item }) => {
        const Icon = DOCUMENT_ICON[item.kind] ?? FileText;
        return {
          label: `Open ${item.name}`,
          title: documentTitle(item),
          icon: <Icon size={13} aria-hidden className="shrink-0" />,
          actions: <>
            <IconAction label={`Reveal ${item.name}`} title="Reveal in file manager" icon={FolderOpen} onClick={() => void openLocalArtifact(item.path).then(() => setError(null), (failure: unknown) => setError(String(failure)))} />
            <ChatAction compact label={`Ask about ${item.name}`} text="Ask" onClick={() => insertIntoComposer(tabId, { attachments: [item.path] })} />
          </>,
        };
      }} />
  </PanelPage>;
}
