import { useEffect, useRef, useState } from "react";
import { FileCode, FileText, Film, FolderOpen, Image } from "lucide-react";
import { ChatBlockHostProvider, RichMarkdown, localArtifactOpeningAvailable, openLocalArtifact } from "@codeterm/chat-ui";
import { formatCompact } from "../../lib/formatCompact";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import type { ChatDocument } from "./chatDocuments";
import { chatDocumentsStore, useChatDocumentSnapshot, useChatDocuments } from "./chatDocumentsStore";
import { insertIntoComposer } from "./composerInsertion";
import { usePinnedBlocks, useTabChatBlockHost } from "./pinnedBlocks";
import { DocumentDetail } from "./tabPanel/DocumentDetail";
import { ChatAction, IconAction, PanelEmpty, PanelPage } from "./tabPanel/PanelParts";
import { PanelFileTree } from "./tabPanel/PanelFileTree";
import { buildPathTree, shortenPath } from "./tabPanel/pathTree";
import { resolveDetail, useTabPanelNavigation } from "./tabPanel/tabPanelNavigation";

export function DocumentsButton({ tabId }: { tabId: string }) {
  const count = useChatDocuments(tabId).length + usePinnedBlocks(tabId).length;
  const entry = usePlanSidePanelEntry();
  if (!count) return null;
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView !== "changes";
  return <button type="button" aria-label={`Documents (${count})`} aria-expanded={open} title={`Documents (${count})`}
    className="ct-bar-pill"
    onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId })}>
    <FileText size={14} aria-hidden />{formatCompact(count)}
  </button>;
}

const DOCUMENT_ICON = { image: Image, video: Film, page: FileCode, document: FileText } as const;

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
  const pins = usePinnedBlocks(tabId);
  if (active) return <DocumentDetail key={active.path} tabId={tabId} document={active} home={home} onBack={back} />;
  if (!documents.length && !pins.length) return <PanelPage><PanelEmpty icon={FileText}>Documents the agent shows in this tab appear here.</PanelEmpty></PanelPage>;
  const nodes = buildPathTree(documents.map((document) => ({ id: document.path, path: shortenPath(document.path, home), item: document })));
  return <PanelPage>
    {error && <p role="alert" className="tab-panel__note">{error}</p>}
    <PinnedBlocks tabId={tabId} pins={pins} />
    {documents.length > 0 && <PanelFileTree label="Documents" nodes={nodes} current={returnTo.current} onOpen={(node) => dispatch({ type: "open", view: "documents", id: node.id })}
      leaf={({ item }) => {
        const Icon = DOCUMENT_ICON[item.kind] ?? FileText;
        return {
          label: `Open ${item.name}`,
          title: documentTitle(item),
          icon: <Icon size={13} aria-hidden className="shrink-0" />,
          actions: <>
            <IconAction label={`Reveal ${item.name}`} title={localArtifactOpeningAvailable() ? "Reveal in file manager" : "Revealing files is available only in the desktop app"} disabled={!localArtifactOpeningAvailable()} icon={FolderOpen} onClick={() => void openLocalArtifact(item.path).then(() => setError(null), (failure: unknown) => setError(String(failure)))} />
            <ChatAction compact label={`Ask about ${item.name}`} text="Ask" onClick={() => insertIntoComposer(tabId, { kind: "document", path: item.path })} />
          </>,
        };
      }} />}
  </PanelPage>;
}

/** Pinned chat blocks stay live here: each re-reads its real state through the same block renderer as the chat. */
function PinnedBlocks({ tabId, pins }: { tabId: string; pins: readonly string[] }) {
  const host = useTabChatBlockHost(tabId);
  if (!pins.length) return null;
  return <section aria-label="Pinned blocks" data-testid="tab-panel-pinned" className="tab-panel__pinned">
    <ChatBlockHostProvider value={host}>
      {pins.map((source) => <RichMarkdown key={source} text={source} />)}
    </ChatBlockHostProvider>
  </section>;
}
