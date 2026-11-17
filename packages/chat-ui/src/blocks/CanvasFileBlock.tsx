import { useContext, useEffect, useMemo } from "react";
import { DS_CANVAS_FILE_FORMAT, isDsCanvasFilePath } from "@codeterm/chat-core";
import { useLocalArtifactTabId } from "../LocalArtifactScope";
import type { LocalPreviewFile } from "../localArtifactLoader";
import { CanvasPreview } from "./CanvasBlock";
import { CanvasNotice } from "./CanvasNotice";
import type { CanvasEditor } from "./canvasEditing";
import { CanvasSourceContext } from "./canvasFilePort";
import { DsRenderContext } from "./DsRenderContext";
import { useCanvasFile } from "./useCanvasFile";

/**
 * A `<ds-canvas src>` file: the same board as an inline canvas, read through the local preview
 * guard, updated in place when the file changes, and editable in its viewer by its local owner.
 */
export function CanvasFileBlock(props: { src: string; view?: string; fullscreenRequest?: number; onLoaded?: (file: LocalPreviewFile | null) => void }) {
  if (!isDsCanvasFilePath(props.src)) return <CanvasNotice text="This canvas file is not available." reason={`A canvas file is named NAME.canvas.json, and its first field is "format": "${DS_CANVAS_FILE_FORMAT}".`} />;
  return <LoadedCanvasFile {...props} />;
}

const LAST_GOOD = "This canvas file has an error, so this is its last good version.";

function LoadedCanvasFile({ src, view, fullscreenRequest = 0, onLoaded }: { src: string; view?: string; fullscreenRequest?: number; onLoaded?: (file: LocalPreviewFile | null) => void }) {
  const tabId = useLocalArtifactTabId();
  const scope = useContext(DsRenderContext);
  const file = useCanvasFile(src, tabId);
  const { session, writable, state } = file;
  const invalid = file.parsed !== null && !file.parsed.ok ? file.parsed.reason : null;
  const readOnly = Boolean(scope.streaming || scope.cancelled || scope.proseOnly) || invalid !== null;
  const editor = useMemo<CanvasEditor | undefined>(() => writable && !readOnly ? {
    commit: (edit) => session.commit(edit),
    undo: () => session.undo(),
    redo: () => session.redo(),
    hold: () => session.hold(),
    retry: () => session.retry(),
    failure: state.failure,
  } : undefined, [writable, readOnly, session, state.failure]);
  useEffect(() => { onLoaded?.(state.text === null ? null : { data: state.text, media_type: "application/json" }); }, [onLoaded, state.text]);
  if (state.text === null) {
    if (file.read.status === "error") return <CanvasNotice text="This canvas file is not available." reason={file.read.reason} />;
    return <div role="status" aria-label="Loading canvas" data-testid="chat-ds-canvas" className="my-2" style={{ height: 320 }} />;
  }
  // Over the last good board the sentence says what is shown; with no good version it says nothing can be.
  const stale = invalid !== null && file.canvas ? { text: LAST_GOOD, reason: invalid } : undefined;
  return <>
    {invalid !== null && <CanvasNotice text={file.canvas ? LAST_GOOD : "This canvas could not be shown because its description is not valid."} reason={invalid} />}
    {file.canvas && <CanvasSourceContext value={src}>
      <CanvasPreview canvas={file.canvas} view={view} editor={editor} fullscreenRequest={fullscreenRequest} details={file.unwatched ?? undefined} notice={stale} />
    </CanvasSourceContext>}
  </>;
}
