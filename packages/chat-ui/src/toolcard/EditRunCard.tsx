import type { FileDiff, ToolEvent } from "@codeterm/chat-core";
import { ChevronRight, Copy, Maximize2 } from "lucide-react";
import { CardChevron } from "../CompactCard";
import { useId, useMemo, useState } from "react";
import { ChatCardMenu, copyPlainText } from "../ChatCardMenu";
import { useChatBlockHost } from "../blocks/ChatBlockHost";
import { COMMAND_TREE_CAP, LINE_CLASS } from "./CommandTree";
import { DiffHunkLines, FilePathLabel, changedLines, useFileChangeRaw } from "./FileDiffCards";
import { StatusGlyph, statusName } from "./ToolActivityStream";
import { toolKindVisual } from "./toolKindVisual";
import { toolDurationLabel } from "./toolDuration";
import { ToolCallProgress } from "./ToolCallProgress";

const QUIET = "var(--ct-muted-fg)";
const RAIL = { color: QUIET, opacity: 0.6 };

/** One file of a run: every edit to the same path, in order. */
interface RunFile {
  id: string;
  path: string;
  movePath?: string;
  kind?: FileDiff["kind"];
  additions: number;
  deletions: number;
  hunks: FileDiff["hunks"];
  status: ToolEvent["status"];
  durationMs?: number;
  message: ToolEvent["message"];
  detailsAvailable: boolean;
}

function runFiles(members: ToolEvent[]): RunFile[] {
  return members.flatMap<RunFile>((member) => {
    const diffs = member.fileDiffs ?? [];
    if (!diffs.length) return [{
      id: `${member.id}:0`, path: member.message.toolLocations?.[0]?.path ?? "",
      additions: 0, deletions: 0, hunks: [], status: member.status, durationMs: member.durationMs,
      message: member.message, detailsAvailable: false,
    }];
    return diffs.map((diff, index) => ({
      id: `${member.id}:${index}`,
      path: diff.path,
      movePath: diff.movePath,
      kind: diff.kind,
      additions: diff.additions,
      deletions: diff.deletions,
      hunks: diff.hunks,
      status: member.status,
      durationMs: member.durationMs,
      message: member.message,
      detailsAvailable: true,
    }));
  });
}

function Counts({ additions, deletions, kind }: { additions: number; deletions: number; kind?: FileDiff["kind"] }) {
  return <span data-testid="edit-run-counts" className="shrink-0 font-mono text-[10.5px]">
    {kind !== "delete" && <span style={{ color: "var(--ct-green)" }}>+{additions}</span>}
    {kind !== "add" && <>{kind !== "delete" && <span> </span>}<span style={{ color: "var(--ct-red)" }}>−{deletions}</span></>}
  </span>;
}

function FileDetail({ file }: { file: RunFile }) {
  if (!file.detailsAvailable) return <div className="px-1 font-mono text-[11px]" style={{ color: QUIET }}>Edit details unavailable</div>;
  return file.hunks.length === 0
    ? <div className="px-1 font-mono text-[11px]" style={{ color: QUIET }}>{file.kind === "delete" ? "File deleted" : "Empty file"}</div>
    : <div data-testid="edit-run-diff" className="overflow-x-auto rounded border py-0.5" style={{ borderColor: "var(--ct-border)", background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)" }}>
      {file.hunks.map((hunk, index) => <div key={index}>
        <div className="px-3 py-0.5 font-mono text-[10.5px]" style={{ color: QUIET }}>{changedLines(hunk)}</div>
        <DiffHunkLines hunk={hunk} path={file.movePath ?? file.path} />
      </div>)}
    </div>;
}

/** Back-to-back file edits as one line with a tree of files, in the same shape as a command group. */
export function EditRunCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: (next: boolean) => void }) {
  const members = event.editRun ?? [];
  const files = useMemo(() => runFiles(members), [members]);
  const raw = useFileChangeRaw(members);
  const [openId, setOpenId] = useState<string | null>(null);
  const host = useChatBlockHost();
  const regionId = useId();
  const visual = toolKindVisual.edit;
  const Glyph = visual.glyph;
  const additions = files.reduce((sum, file) => sum + file.additions, 0);
  const deletions = files.reduce((sum, file) => sum + file.deletions, 0);
  const distinctFileCount = new Set(files.map((file) => file.movePath ?? file.path)).size;
  const hasNonEdit = files.some((file) => file.kind === "add" || file.kind === "delete" || file.kind === "rename");
  const title = hasNonEdit ? "File changes" : `Edited ${distinctFileCount} ${distinctFileCount === 1 ? "file" : "files"}`;
  const visible = expanded || files.length <= COMMAND_TREE_CAP + 1 ? files.length : COMMAND_TREE_CAP;
  const hidden = files.length - visible;
  const visibleFiles = hidden > 0 && event.liveEditRun
    ? [...files.slice(0, visible - 1), files[files.length - 1]] : files.slice(0, visible);
  const openFile = host.openFileChange;
  const copyPaths = { label: "Copy paths", icon: <Copy size={14} aria-hidden />,
    onSelect: () => copyPlainText(files.map((file) => file.movePath ?? file.path).join("\n")) };

  return (
    <section
      data-testid="chat-edit-run"
      data-tool-status={event.status}
      data-expanded={expanded}
      aria-label={`${title}, +${additions} −${deletions}`}
      className="ct-card ct-command-inline mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md"
    >
      <div className="ct-command-inline__head flex w-full min-w-0 items-center gap-2 text-xs">
        <button
          type="button"
          data-testid="chat-edit-run-toggle"
          aria-expanded={expanded}
          aria-controls={regionId}
          aria-label={`${expanded ? "Hide" : "Show"} all files in ${title}`}
          onClick={() => onToggle(!expanded)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <Glyph aria-hidden size={16} className="shrink-0" style={{ color: visual.color }} />
          <span data-testid="chat-edit-run-title" className="min-w-0 truncate text-[12.5px]" style={{ color: "var(--ct-fg)" }}>{title}</span>
          {event.status !== "pending" && <Counts additions={additions} deletions={deletions} />}
          <span className="flex-1" />
        </button>
        <span className="ct-tool-status-slot">
          <span role="status" aria-label={statusName(event.status)} className="ct-tool-status-icon flex items-center justify-center">
            <StatusGlyph status={event.status} />
          </span>
          <ChatCardMenu label={`Actions for ${title}`} items={raw.item ? [copyPaths, raw.item] : [copyPaths]} joinRow />
        </span>
        <CardChevron expanded={expanded} label={`${expanded ? "Collapse" : "Expand"} edited files`} onToggle={() => onToggle(!expanded)} />
      </div>
      <ol id={regionId} data-testid="chat-edit-run-files" aria-label={`${files.length} file changes`} className="m-0 min-w-0 list-none py-0.5 pb-1 pl-4 pr-2 font-mono text-[11.5px]">
        {visibleFiles.map((file, index) => {
          const path = file.movePath ?? file.path;
          const open = openId === file.id;
          const running = file.status === "pending" && file.message.toolStatus === "running";
          const verb = running ? "Editing" : file.kind === "add" ? "Created" : file.kind === "delete" ? "Deleted" : "Edited";
          const FileGlyph = visual.glyph;
          return <li key={file.id} data-testid="chat-edit-run-file" className="group/file min-w-0">
            <div className="flex min-w-0 items-center">
              <button
                type="button"
                aria-expanded={open}
                aria-label={`${open ? "Hide" : "Show"} diff for ${path || "this edit"}, change ${index + 1}`}
                onClick={() => setOpenId(open ? null : file.id)}
                className={`${LINE_CLASS} gap-2`}
              >
                <FileGlyph aria-hidden size={14} className="shrink-0" style={{ color: visual.color }} />
                <span title={path || undefined} className="min-w-0 truncate"><span style={{ color: QUIET }}>{verb} </span>{path ? <FilePathLabel path={file.path} /> : "files"}{file.movePath && <> → <FilePathLabel path={file.movePath} /></>}</span>
                {!running && file.detailsAvailable && <Counts additions={file.additions} deletions={file.deletions} kind={file.kind} />}
                {running ? <ToolCallProgress message={file.message} /> : file.durationMs !== undefined && <span data-testid="edit-run-duration" aria-label={`Duration ${file.durationMs} milliseconds`} title={`${file.durationMs}ms`} className="shrink-0 font-mono text-[10.5px]" style={{ color: QUIET }}>{toolDurationLabel(file.durationMs)}</span>}
                <span role="status" aria-label={statusName(file.status)} className="flex size-4 shrink-0 items-center justify-center" style={{ color: file.status === "success" ? "var(--ct-green)" : QUIET }}><StatusGlyph status={file.status} /></span>
                <ChevronRight aria-hidden size={14} data-testid="edit-run-chevron" className={`shrink-0 transition-transform ${open ? "rotate-90" : ""}`} style={{ color: QUIET }} />
              </button>
              {openFile && path && <button
                type="button"
                aria-label={`View changes in ${path}`}
                title="View changes"
                onClick={() => openFile(path, "diff")}
                className="ml-1 flex size-5 shrink-0 items-center justify-center rounded opacity-0 transition-opacity hover:bg-[var(--ct-surface)] focus-visible:opacity-100 group-hover/file:opacity-100 [@media(hover:none)]:opacity-100"
                style={{ color: QUIET }}
              ><Maximize2 aria-hidden size={12} /></button>}
            </div>
            {open && <div className="mb-1 ml-[3ch] mt-0.5 min-w-0"><FileDetail file={file} /></div>}
          </li>;
        })}
        {hidden > 0 && <li className="min-w-0">
          <button type="button" data-testid="chat-edit-run-more" onClick={() => onToggle(true)} className={LINE_CLASS} style={{ color: QUIET }}>
            <span aria-hidden="true" style={RAIL}>└</span>
            <span>{`Show all ${hidden} remaining ${hidden === 1 ? "file" : "files"}`}</span>
          </button>
        </li>}
      </ol>
      {raw.body}
    </section>
  );
}
