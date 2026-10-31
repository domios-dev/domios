import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { useCallback, useState } from "react";
import { CodeBlock, knownLanguageFromPath, LocalArtifactBlock, LocalArtifactScope, RichMarkdown } from "@codeterm/chat-ui";
import { changePreviewKind } from "./changePreviewModel";

function ChangeCodeView({ text, path }: { text: string; path: string }) {
  const [wrap, setWrap] = useState(false);
  return <>
    <button type="button" className="ct-small-pill mb-2" aria-pressed={wrap} onClick={() => setWrap((value) => !value)}>Wrap lines</button>
    <CodeBlock text={text} language={knownLanguageFromPath(path)} maxHeight="none" wrapLines={wrap} showLineNumbers />
  </>;
}

const renderMarkdown = (text: string) => <RichMarkdown text={text} />;
const renderError = (error: string) => <InlineErrorNotice error={error} operation="previewing this file" />;

export function ChangeFileView({ tabId, target, deleted, onDiff, fullscreenRequest = 0 }: { tabId: string; target: string | null; deleted: boolean; onDiff: () => void; fullscreenRequest?: number }) {
  const renderText = useCallback((text: string) => <ChangeCodeView text={text} path={target ?? ""} />, [target]);
  return <div className="tab-panel__reader">
    {deleted ? <p role="status" className="tab-panel__note">This file was deleted. Read its last recorded diff.</p>
      : !target ? <p role="status" className="tab-panel__note">The current file location is unknown.</p>
        : <LocalArtifactScope tabId={tabId}>
          <p className="tab-panel__note">Current file on disk</p>
          <LocalArtifactBlock key={target} bare directive={{ path: target, kind: changePreviewKind(target) }} fullscreenRequest={fullscreenRequest}
            renderError={renderError} renderMarkdown={renderMarkdown} renderText={renderText} />
        </LocalArtifactScope>}
    <button type="button" className="ct-small-pill" onClick={onDiff}>Show recorded diff</button>
  </div>;
}
