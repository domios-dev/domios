import { useState } from "react";
import { LocalArtifactBlock, RichMarkdown } from "@codeterm/chat-ui";
import type { AutomationIdentity } from "./types";
import { filePreviewKind, revealFileLabel, type NodeFile } from "./nodeFilesModel";
import { nodeFilesAdapter, useNodeFiles, type NodeFilesAdapter } from "./useNodeFiles";
import { AUTOMATION_TOOL } from "./styles";

export function NodeFiles({ identity, bucket = null, handler = null, adapter = nodeFilesAdapter }: {
  identity: AutomationIdentity; bucket?: string | null; handler?: string | null; adapter?: NodeFilesAdapter;
}) {
  const result = useNodeFiles(adapter, identity, bucket, handler);
  const [opened, setOpened] = useState<NodeFile | null>(null);
  const [failed, setFailed] = useState(false);
  const revealLabel = revealFileLabel(navigator.platform);
  const perform = (action: Promise<unknown>) => { setFailed(false); void action.catch(() => setFailed(true)); };
  const open = (file: NodeFile) => {
    if (filePreviewKind(file.path)) setOpened(file);
    else perform(adapter.open(file.path));
  };
  const preview = opened && filePreviewKind(opened.path);
  return <section aria-label="Node files" className="ct-detail__block">
    <h4 className="ct-detail__heading">Files</h4>
    {!result && <p role="status" className="ct-job-note">Reading file references…</p>}
    {result?.failed && <p role="status" className="ct-job-note">File references are unavailable.</p>}
    {result && !result.failed && !result.files.length && <p className="ct-job-note">No literal file paths in this definition.</p>}
    <ul className="ct-detail__sections m-0 list-none p-0">{result?.files.map((file) => <li key={file.path} className={`ct-detail__section${file.exists ? "" : " text-[var(--ct-muted-fg)] opacity-50"}`}>
      <p className="m-0 break-words font-semibold">{file.name}{!file.exists && <span> · not found</span>}</p>
      <p className="ct-detail__mono m-0 text-[var(--ct-muted-fg)]" title={file.path}>{file.short_path}</p>
      <div className="flex flex-wrap gap-1.5">
        <button type="button" className={AUTOMATION_TOOL} disabled={!file.exists} aria-label={`Open ${file.name}`} onClick={() => open(file)}>Open</button>
        <button type="button" className={AUTOMATION_TOOL} disabled={!file.exists} aria-label={`${revealLabel}: ${file.name}`} onClick={() => perform(adapter.reveal(file.path))}>{revealLabel}</button>
      </div>
    </li>)}</ul>
    {failed && <p role="alert" className="ct-job-note">The file could not be opened.</p>}
    {opened && preview && <div className="flex flex-col items-start gap-2">
      <button type="button" className={AUTOMATION_TOOL} aria-label="Close file preview" onClick={() => setOpened(null)}>Close preview</button>
      <LocalArtifactBlock directive={{ kind: preview, path: opened.path }} renderMarkdown={(source) => <RichMarkdown text={source} localArtifacts={false} />} />
    </div>}
  </section>;
}
