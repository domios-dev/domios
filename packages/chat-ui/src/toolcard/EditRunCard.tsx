import type { FileDiff, ToolEvent } from "@codeterm/chat-core";
import { Copy, Maximize2 } from "lucide-react";
import { CardChevron } from "../CompactCard";
import { useId, useMemo, useState } from "react";
import { ChatCardMenu, copyPlainText } from "../ChatCardMenu";
import { useChatBlockHost } from "../blocks/ChatBlockHost";
import { ToolOutputContent } from "../ToolOutputContent";
import { COMMAND_TREE_CAP, LINE_CLASS } from "./CommandTree";
import { DiffHunkLines, changedLines } from "./FileDiffCards";
import { StatusGlyph, statusName } from "./ToolActivityStream";
import { toolKindVisual } from "./toolKindVisual";

const QUIET = "var(--ct-muted-fg)";
const RAIL = { color: QUIET, opacity: 0.6 };

/** One file of a run: every edit to the same path, in order. */
interface RunFile {
  path: string;
  movePath?: string;
  kind?: FileDiff["kind"];
  additions: number;
  deletions: number;
  edits: number;
  hunks: FileDiff["hunks"];
}

function runFiles(members: ToolEvent[]): RunFile[] {
  const files = new Map<string, RunFile>();
  for (const member of members) {
    for (const diff of member.fileDiffs ?? []) {
      const path = diff.movePath ?? diff.path;
      const file = files.get(path);
      if (file) {
        file.additions += diff.additions;
        file.deletions += diff.deletions;
        file.edits += 1;
        file.hunks = [...file.hunks, ...diff.hunks];
        if (diff.kind === "delete") file.kind = "delete";
      } else {
        files.set(path, { path: diff.path, movePath: diff.movePath, kind: diff.kind, additions: diff.additions, deletions: diff.deletions, edits: 1, hunks: diff.hunks });
      }
    }
  }
  return [...files.values()];
}

function PathLabel({ path }: { path: string }) {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return <>
    {parts.length > 1 && <span style={{ color: QUIET }}>{parts[parts.length - 2]}/</span>}
    <span style={{ color: "var(--ct-fg)" }}>{parts[parts.length - 1] ?? path}</span>
  </>;
}

function Counts({ additions, deletions }: { additions: number; deletions: number }) {
  return <span data-testid="edit-run-counts" className="shrink-0 font-mono text-[10.5px]">
    <span style={{ color: "var(--ct-green)" }}>+{additions}</span>{" "}
    <span style={{ color: "var(--ct-red)" }}>−{deletions}</span>
  </span>;
}

function FileDetail({ file }: { file: RunFile }) {
  if (file.hunks.length === 0) return <div className="px-1 font-mono text-[11px]" style={{ color: QUIET }}>{file.kind === "delete" ? "File deleted" : "Empty file"}</div>;
  return <div data-testid="edit-run-diff" className="overflow-x-auto rounded border py-0.5" style={{ borderColor: "var(--ct-border)", background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)" }}>
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
  const [openPath, setOpenPath] = useState<string | null>(null);
  const host = useChatBlockHost();
  const regionId = useId();
  const visual = toolKindVisual.edit;
  const Glyph = visual.glyph;
  const additions = files.reduce((sum, file) => sum + file.additions, 0);
  const deletions = files.reduce((sum, file) => sum + file.deletions, 0);
  const title = `Edited ${files.length} ${files.length === 1 ? "file" : "files"}`;
  const visible = expanded || files.length <= COMMAND_TREE_CAP + 1 ? files.length : COMMAND_TREE_CAP;
  const hidden = files.length - visible;
  const failures = members.filter((member) => member.status === "error" && member.result);
  const openFile = host.openFileChange;

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
          <Counts additions={additions} deletions={deletions} />
          <span className="flex-1" />
        </button>
        <span className="ct-tool-status-slot">
          <span role="status" aria-label={statusName(event.status)} className="ct-tool-status-icon flex items-center justify-center">
            <StatusGlyph status={event.status} />
          </span>
          <ChatCardMenu label={`Actions for ${title}`} items={[{
            label: "Copy paths", icon: <Copy size={14} aria-hidden />,
            onSelect: () => copyPlainText(files.map((file) => file.movePath ?? file.path).join("\n")),
          }]} joinRow />
        </span>
        <CardChevron expanded={expanded} label={`${expanded ? "Collapse" : "Expand"} edited files`} onToggle={() => onToggle(!expanded)} />
      </div>
      <ol id={regionId} data-testid="chat-edit-run-files" aria-label={`${files.length} edited files`} className="m-0 min-w-0 list-none py-0.5 pb-1 pl-4 pr-2 font-mono text-[11.5px]">
        {files.slice(0, visible).map((file, index) => {
          const path = file.movePath ?? file.path;
          const open = openPath === path;
          const last = index === files.length - 1;
          return <li key={path} data-testid="chat-edit-run-file" className="group/file min-w-0">
            <div className="flex min-w-0 items-center">
              <button
                type="button"
                aria-expanded={open}
                aria-label={`${open ? "Hide" : "Show"} diff for ${path}`}
                onClick={() => setOpenPath(open ? null : path)}
                className={LINE_CLASS}
              >
                <span aria-hidden="true" className="shrink-0" style={RAIL}>{last ? "└" : "├"}</span>
                <span title={path} className="min-w-0 truncate"><PathLabel path={file.path} />{file.movePath && <> → <PathLabel path={file.movePath} /></>}</span>
                {file.edits > 1 && <span className="shrink-0 text-[10px]" style={{ color: QUIET }}>×{file.edits}</span>}
                {file.kind === "add" && <span className="shrink-0 text-[10px]" style={{ color: QUIET }}>new</span>}
                {file.kind === "delete" && <span className="shrink-0 text-[10px]" style={{ color: "var(--ct-red)", opacity: 0.75 }}>deleted</span>}
                <Counts additions={file.additions} deletions={file.deletions} />
              </button>
              {openFile && <button
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
      {expanded && failures.length > 0 && <div data-testid="toolcard-result" className="px-3 pb-2">
        {failures.map((member) => <ToolOutputContent key={member.id} text={member.result ?? ""} maxHeight="12rem" />)}
      </div>}
    </section>
  );
}
