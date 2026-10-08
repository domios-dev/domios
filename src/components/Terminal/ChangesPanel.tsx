import { useState, type RefObject } from "react";
import { FileDiff } from "lucide-react";
import { FileDiffCards } from "@codeterm/chat-ui";
import { timestampMs, type RecordedFileChange, type ToolEvent } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";
import { requestChatMessageReveal, useChatReveal } from "../../lib/chatMessageReveal";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import { SidePanelFrame } from "../Canvas/planView/SidePanelFrame";
import type { PresenceState } from "../Canvas/usePresence";
import { useChatChanges } from "./chatChangesStore";
import { TabPanelNavigation } from "./TabPanelNavigation";
import { changesDirectories, changesFilePath, changesTotals } from "./changesViewModel";

export function ChangesButton({ tabId }: { tabId: string }) {
  const changes = useChatChanges(tabId);
  const entry = usePlanSidePanelEntry();
  if (!changes.cumulative.length) return null;
  const { additions, deletions } = changesTotals(changes.cumulative);
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView === "changes";
  return <button type="button" aria-label={`Changes (+${additions} -${deletions})`} aria-expanded={open}
    title="Recorded edits" className="inline-flex items-center gap-1 rounded px-1.5 text-xs text-[var(--ct-muted-fg)]"
    onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId, tabView: "changes" })}>
    <FileDiff size={14} aria-hidden /><span className="text-[var(--ct-green)]">+{additions}</span><span className="text-[var(--ct-red)]">-{deletions}</span>
  </button>;
}

function FileList({ files, selected, onSelect }: { files: RecordedFileChange[]; selected: string | null; onSelect: (path: string) => void }) {
  const directories = changesDirectories(files);
  return <nav aria-label="Changed files" className="my-3 text-xs">
    {directories.map((directory) => <details key={directory.path} open>
      <summary className="py-1 font-mono">{directory.path} <span className="text-[var(--ct-green)]">+{directory.additions}</span> <span className="text-[var(--ct-red)]">-{directory.deletions}</span></summary>
      <ul className="ml-3 border-l border-[var(--ct-border)] pl-2">
        {directory.files.map((file) => <li key={file.movePath ?? file.path}><button type="button" aria-pressed={selected === (file.movePath ?? file.path)}
          aria-label={`Show changes for ${file.movePath ?? file.path}`} onClick={() => onSelect(file.movePath ?? file.path)}
          className="flex w-full items-center gap-2 rounded p-1 text-left hover:bg-[var(--ct-surface)]">
          <span className="min-w-0 flex-1 truncate" title={file.path}>{(file.movePath ?? file.path).replace(/\\/g, "/").split("/").pop()}</span>
          <span>{file.kind}</span><span className="text-[var(--ct-green)]">+{file.additions}</span><span className="text-[var(--ct-red)]">-{file.deletions}</span>
        </button></li>)}
      </ul>
    </details>)}
  </nav>;
}

function FileDetail({ file, currentFile, tabId, cwd }: { file: RecordedFileChange; currentFile: RecordedFileChange; tabId: string; cwd?: string | null }) {
  const [error, setError] = useState<string | null>(null);
  const reveal = useChatReveal();
  const deleted = currentFile.kind === "delete";
  const path = changesFilePath(currentFile, cwd);
  const perform = (command: "open_folder" | "reveal_file") => {
    if (path) void invoke(command, { path }).then(() => setError(null), (failure: unknown) => setError(String(failure)));
  };
  const event: ToolEvent = { id: file.path, phase: "result", status: "success", kind: "edit", summary: file.path,
    fileDiffs: file.diffs, message: { id: file.path, type: "tool_result", content: "" } };
  return <section aria-label={`Recorded edits for ${file.path}`} className="mt-3">
    <h3 className="break-all font-mono text-xs">{file.movePath ? `${file.path} → ${file.movePath}` : file.path}</h3>
    <div className="my-2 flex flex-wrap gap-3 text-xs">
      <button type="button" disabled={!path || deleted} onClick={() => perform("open_folder")}>Open file</button>
      <button type="button" disabled={!path || deleted} onClick={() => perform("reveal_file")}>Show in Explorer/Finder</button>
      {file.turnIds.map((turnId, index) => <button key={turnId} type="button" onClick={() => { setClientTabUiMode(tabId, "chat"); requestChatMessageReveal(tabId, turnId); }}>Jump to turn{file.turnIds.length > 1 ? ` ${index + 1}` : ""}</button>)}
    </div>
    {!path && <p>The tab's working directory is unavailable, so this relative path cannot be opened.</p>}
    {deleted && <p>This file was deleted. Its recorded edits and source turns remain available below.</p>}
    {currentFile.movePath && <p>The file moved to {currentFile.movePath}. File actions use that destination.</p>}
    {file.binary && <p>Binary file: a text diff is unavailable. Open the file to inspect it.</p>}
    {file.truncated && <p>The recorded diff is truncated. Open the file or jump to its turn for more context.</p>}
    {!file.binary && file.diffs.every((diff) => !diff.hunks.length) && <p>No text hunks were recorded for this edit.</p>}
    {error && <p role="alert">{error}</p>}
    {reveal?.tabId === tabId && file.turnIds.includes(reveal.messageId) && (reveal.outcome === "missing" || reveal.outcome === "no_chat") && <p role="status">This turn is outside the loaded chat history. Load older messages in chat to find it.</p>}
    {file.diffs.some((diff) => diff.hunks.length > 0) && <FileDiffCards event={event} initiallyExpanded />}
  </section>;
}

export function ChangesPanel({ tabId, cwd, panelRef, state, onClose }: { tabId: string; cwd?: string | null; panelRef: RefObject<HTMLElement | null>; state: PresenceState; onClose: () => void }) {
  const changes = useChatChanges(tabId);
  const [scope, setScope] = useState("cumulative");
  const [turnId, setTurnId] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const turns = [...changes.turns].reverse();
  const turn = turns.find((item) => item.turnId === turnId) ?? turns[0];
  const files = scope === "cumulative" ? changes.cumulative : turn?.files ?? [];
  const active = files.find((file) => (file.movePath ?? file.path) === selected);
  const currentFile = active && changes.cumulative.find((file) => file.diffs.includes(active.diffs[0]));
  const totals = changesTotals(files);
  return <SidePanelFrame panelRef={panelRef} state={state} label="Changes" onClose={onClose} contentKey={tabId} title={<h2 className="plan-side-panel__title">Recorded edits</h2>}>
    <TabPanelNavigation tabId={tabId} view="changes" />
    <p className="mb-3 text-xs text-[var(--ct-muted-fg)]">Edits recorded in the loaded transcript. Shell-made changes are not included. Counts sum recorded edits; cumulative diffs show their history.</p>
    <div role="group" aria-label="Changes scope" className="flex gap-3 text-sm">
      <button type="button" aria-pressed={scope === "cumulative"} onClick={() => { setScope("cumulative"); setSelected(null); }}>Cumulative</button>
      <button type="button" aria-pressed={scope === "turn"} onClick={() => { setScope("turn"); setSelected(null); }}>Per turn</button>
    </div>
    <p role="status" className="mt-2 text-xs">{files.length} files · <span className="text-[var(--ct-green)]">+{totals.additions}</span> <span className="text-[var(--ct-red)]">-{totals.deletions}</span></p>
    {scope === "turn" && turns.length > 0 && <label className="mt-2 block text-xs">Turn <select aria-label="Turn" value={turn?.turnId} onChange={(event) => { setTurnId(event.target.value); setSelected(null); }}>
      {turns.map((item, index) => <option key={item.turnId} value={item.turnId}>Turn {turns.length - index}{timestampMs(item.at) != null ? ` · ${new Date(timestampMs(item.at) ?? 0).toLocaleString()}` : ""} ({item.files.length} files)</option>)}
    </select></label>}
    {!changes.turns.length ? <p className="mt-3">No turns yet. Send a message to start work in this tab.</p>
      : !files.length ? <p className="mt-3">No recorded edits in this scope. File edits appear when the agent records them; shell changes are not captured.</p>
        : <><FileList files={files} selected={selected} onSelect={setSelected} />
          {active ? <FileDetail key={`${scope}:${turn?.turnId}:${active.movePath ?? active.path}`} file={active} currentFile={currentFile ?? active} tabId={tabId} cwd={cwd} /> : <p className="text-xs">Select a file to inspect its recorded diffs.</p>}</>}
  </SidePanelFrame>;
}
