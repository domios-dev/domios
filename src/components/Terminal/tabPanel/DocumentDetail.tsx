import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { useRef, useState } from "react";
import { ClipboardCopy, FolderOpen, Link2, Maximize2 } from "lucide-react";
import { LocalArtifactScope, LocalArtifactBlock, localArtifactOpeningAvailable, RichMarkdown, openLocalArtifact, type LocalPreviewFile } from "@codeterm/chat-ui";
import type { ChatDocument } from "../chatDocuments";
import { insertIntoComposer } from "../composerInsertion";
import { BackBar, ChatAction, CopyAction, IconAction, PanelPage, copyText } from "./PanelParts";
import { shortenPath } from "./pathTree";
import { lineSuffix, locateSelection, quotedSelection } from "./selectionReference";
import { SelectionBar, useReaderSelection } from "./SelectionBar";

export function DocumentDetail({ tabId, document, home, onBack }: { tabId: string; document: ChatDocument; home: string | null; onBack: () => void }) {
  const [fullscreenRequest, setFullscreenRequest] = useState(0);
  const [source, setSource] = useState<LocalPreviewFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const readerRef = useRef<HTMLDivElement>(null);
  const selection = useReaderSelection(readerRef);
  const path = shortenPath(document.path, home);
  const text = source && document.kind !== "image" ? source.data : null;
  const fail = (failure: unknown) => setError(String(failure));
  function addSelection(selected: string) {
    const range = text ? locateSelection(text, selected) : null;
    insertIntoComposer(tabId, { kind: "quote", detail: `${document.name}${lineSuffix(range)}`, content: quotedSelection(document.path, selected, range) });
    window.getSelection()?.removeAllRanges();
  }
  return <PanelPage onBack={onBack} reader bar={<BackBar path={path} onBack={onBack} actions={<>
    <IconAction label={`Fullscreen ${document.name}`} title="Fullscreen" icon={Maximize2} onClick={() => setFullscreenRequest((value) => value + 1)} />
    <IconAction label={`Reveal ${document.name}`} title={localArtifactOpeningAvailable() ? "Reveal in file manager" : "Revealing files is available only in the desktop app"} disabled={!localArtifactOpeningAvailable()} icon={FolderOpen} onClick={() => void openLocalArtifact(document.path).then(() => setError(null), fail)} />
    <CopyAction label="Copy path" icon={Link2} copy={() => copyText(document.path)} />
    <CopyAction label="Copy content" icon={ClipboardCopy} disabled={!text} copy={() => copyText(text ?? "")} />
    <ChatAction label={`Ask about ${document.name}`} text="Ask" onClick={() => insertIntoComposer(tabId, { kind: "document", path: document.path })} />
  </>} />}>
    {error && <InlineErrorNotice error={error} operation="opening this document" blocking />}
    <div ref={readerRef} className="tab-panel__reader">
      <LocalArtifactScope tabId={tabId}><LocalArtifactBlock key={document.path} bare directive={document} fullscreenRequest={fullscreenRequest} fullscreenHostSelector={`[data-tab-id="${tabId}"]`}
        renderMarkdown={(markdown) => <RichMarkdown text={markdown} />} onLoaded={setSource} /></LocalArtifactScope>
      <SelectionBar selection={selection} onCopy={(selected) => void copyText(selected).catch(fail)} onAddToChat={addSelection} />
    </div>
  </PanelPage>;
}
