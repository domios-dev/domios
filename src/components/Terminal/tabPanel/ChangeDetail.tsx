import { useCallback, useState } from "react";
import { ClipboardCopy, ExternalLink, FolderOpen, Info, Maximize2 } from "lucide-react";
import { FileDiffCards } from "@codeterm/chat-ui";
import type { ChangesTurn, RecordedFileChange, ToolEvent } from "@codeterm/chat-core";
import { IS_WEB, invoke } from "../../../lib/bridge";
import { requestChatMessageReveal, useChatReveal } from "../../../lib/chatMessageReveal";
import { setClientTabUiMode } from "../../../lib/clientTabUiMode";
import { hunkHeader } from "@codeterm/chat-core";
import { CHANGE_STATUS, changeContext, changesFilePath, diffCapsuleLabel, hunkCounts, hunkReference, turnNumber, unifiedDiffText } from "../changesViewModel";
import { insertIntoComposer } from "../composerInsertion";
import { Segmented } from "../../shared/Segmented";
import { ChangeFileView } from "./ChangeFileView";
import { ChangeFullscreen } from "./ChangeFullscreen";
import { useTabPanelNavigation } from "./tabPanelNavigation";
import { BackBar, ChangeBar, ChatAction, CopyAction, CountPair, IconAction, PanelPage, StatusGlyph, copyText } from "./PanelParts";

/** Adds one diff capsule holding the recorded change and the file's current location while it still exists. */
export function askAboutChange(tabId: string, file: RecordedFileChange, path: string, cwd?: string | null, current = file) {
  const target = current.kind === "delete" ? null : changesFilePath(current, cwd);
  insertIntoComposer(tabId, { kind: "diff", label: diffCapsuleLabel(path, file), content: changeContext(path, file, target) });
}

export function ChangeDetail({ tabId, file, currentFile, turns, path, cwd, display, onBack }: {
  tabId: string;
  file: RecordedFileChange;
  /** The cumulative record of the same file, whose latest location file actions use. */
  currentFile: RecordedFileChange;
  turns: readonly ChangesTurn[];
  path: string;
  cwd?: string | null;
  display: (path: string) => string;
  onBack: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [navigation, dispatch] = useTabPanelNavigation(tabId);
  const view = navigation.changes.view;
  const [fullscreen, setFullscreen] = useState(false);
  const closeFullscreen = useCallback(() => setFullscreen(false), []);
  const reveal = useChatReveal();
  const deleted = currentFile.kind === "delete";
  const target = changesFilePath(currentFile, cwd);
  const perform = (command: "open_folder" | "reveal_file") => {
    if (target) void invoke(command, { path: target }).then(() => setError(null), (failure: unknown) => setError(String(failure)));
  };
  const jump = (turnId: string) => { setClientTabUiMode(tabId, "chat"); requestChatMessageReveal(tabId, turnId); };
  const diffs = file.diffs.map((diff) => ({ ...diff, path: display(diff.path), movePath: diff.movePath && display(diff.movePath) }));
  const event: ToolEvent = { id: file.path, phase: "result", status: "success", kind: "edit", summary: file.path,
    fileDiffs: diffs, message: { id: file.path, type: "tool_result", content: "" } };
  const hasHunks = file.diffs.some((diff) => diff.hunks.length > 0);
  const missingTurn = reveal?.tabId === tabId && file.turnIds.includes(reveal.messageId) && (reveal.outcome === "missing" || reveal.outcome === "no_chat");
  const content = view === "file"
    ? <ChangeFileView tabId={tabId} target={target} deleted={deleted} onDiff={() => dispatch({ type: "change-view", view: "diff" })} />
    : <section aria-label={`Recorded edits for ${file.path}`}>
      <div className="tab-panel__summary">
        <StatusGlyph kind={file.kind} />
        <span>{CHANGE_STATUS[file.kind].label}</span>
        <CountPair additions={file.additions} deletions={file.deletions} />
        <ChangeBar additions={file.additions} deletions={file.deletions} />
        <span className="tab-panel__turns" role="group" aria-label="Source turns">
          {file.turnIds.map((turnId) => {
            const number = turnNumber(turns, turnId);
            return <button key={turnId} type="button" className="ct-small-pill tab-panel__turn" aria-label={file.turnIds.length > 1 ? `Jump to turn ${number ?? ""}`.trim() : "Jump to turn"}
              title="Show this turn in chat" onClick={() => jump(turnId)}>{number ? `Turn ${number}` : "Turn"}</button>;
          })}
        </span>
      </div>
      {!target && <p className="tab-panel__note"><Info size={13} aria-hidden />The tab's working directory is unknown, so this relative path cannot be opened.</p>}
      {deleted && <p className="tab-panel__note"><Info size={13} aria-hidden />This file was deleted. Its recorded edits stay below.</p>}
      {currentFile.movePath && <p className="tab-panel__note"><Info size={13} aria-hidden />Moved to {currentFile.movePath}; file actions open the new location.</p>}
      {file.binary && <p className="tab-panel__note"><Info size={13} aria-hidden />Binary file: no text diff. Open the file to inspect it.</p>}
      {file.truncated && <p className="tab-panel__note"><Info size={13} aria-hidden />The recorded diff is truncated. Open the file or jump to its turn for the rest.</p>}
      {!file.binary && !hasHunks && <p className="tab-panel__note"><Info size={13} aria-hidden />No text hunks were recorded for this edit.</p>}
      {error && <p role="alert" className="tab-panel__note">{error}</p>}
      {missingTurn && <p role="status" className="tab-panel__note"><Info size={13} aria-hidden />This turn is outside the loaded chat history. Load older messages in chat to find it.</p>}
      {hasHunks && <div className="tab-panel__diffs">
        <FileDiffCards event={event} initiallyExpanded rawCommand={false} renderHunkActions={(_diff, hunk) => <ChatAction compact text="Add to chat"
          label={`Add hunk ${hunkHeader(hunk)} to chat`} onClick={() => insertIntoComposer(tabId, { kind: "diff", label: diffCapsuleLabel(path, hunkCounts(hunk)), content: hunkReference(path, hunk) })} />} />
      </div>}
    </section>;
  return <PanelPage onBack={onBack} bar={<BackBar path={path} onBack={onBack} actions={<>
    <Segmented aria-label="Change view" value={view} height={26} options={[{ value: "diff", label: "Diff" }, { value: "file", label: "File" }]}
      onChange={(view) => dispatch({ type: "change-view", view })} />
    <IconAction label="Fullscreen" icon={Maximize2} onClick={() => setFullscreen(true)} />
    <IconAction label="Open file" icon={ExternalLink} disabled={IS_WEB || !target || deleted} title={IS_WEB ? "Opening files in the OS is available only in the desktop app" : undefined} onClick={() => perform("open_folder")} />
    <IconAction label="Show in Explorer/Finder" icon={FolderOpen} disabled={IS_WEB || !target || deleted} title={IS_WEB ? "Opening files in the OS is available only in the desktop app" : undefined} onClick={() => perform("reveal_file")} />
    <CopyAction label="Copy diff" icon={ClipboardCopy} disabled={!hasHunks} copy={() => copyText(unifiedDiffText({ ...file, diffs }))} />
    <ChatAction label={`Ask about the change to ${path}`} text="Ask" onClick={() => askAboutChange(tabId, file, path, cwd, currentFile)} />
  </>} />}>
    {error && view === "file" && <p role="alert" className="tab-panel__note">{error}</p>}
    {fullscreen ? <ChangeFullscreen path={path} onClose={closeFullscreen}>{content}</ChangeFullscreen> : content}
  </PanelPage>;
}
