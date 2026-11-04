import { ExpandableReader } from "../ExpandableReader";
import { shellFileOperations, toolCommand, type ToolEvent } from "@codeterm/chat-core";
import { ChevronRight, FileDiff } from "lucide-react";
import { useId, useState } from "react";
import { BoundedMessageContent } from "../BoundedMessageContent";
import { FloatingTooltip } from "../FloatingTooltip";
import { commandCallKey } from "./CommandTree";

type Operation = ReturnType<typeof shellFileOperations>["operations"][number];
const OPEN_PREVIEWS = new Map<string, boolean>();

function ReplacementPreview({ operation, identity }: { operation: Operation; identity: string }) {
  const [open, setOpen] = useState(() => OPEN_PREVIEWS.get(identity) ?? false);
  const regionId = useId();
  const replacement = operation.replacement;
  if (!replacement && operation.content === undefined) return null;
  const count = replacement?.fragments.length ?? 0;
  const previewKind = replacement ? "replacement" : operation.action;
  const filename = operation.path?.split(/[\\/]/).pop() || operation.path;
  const toggle = () => {
    const next = !open;
    if (identity.length <= 2_048) {
      if (OPEN_PREVIEWS.size >= 64) OPEN_PREVIEWS.delete(OPEN_PREVIEWS.keys().next().value as string);
      OPEN_PREVIEWS.set(identity, next);
    }
    setOpen(next);
  };
  return <ExpandableReader label={filename || "File preview"} openerPlacement="header">{(fullscreen, opener) => <div className="ct-reader-surface min-w-0">
    <div className="ct-reader-row flex min-w-0 items-center gap-1.5">
    <FloatingTooltip content={operation.path} focusable={false} className="block min-w-0 flex-1">
      <button type="button" aria-expanded={open} aria-controls={regionId}
        aria-label={`${open ? "Hide" : "Show"} ${previewKind} preview for ${filename}`}
        className="flex w-full min-w-0 items-center gap-1.5 text-left"
        onClick={toggle}>
        <FileDiff size={14} aria-hidden className="shrink-0" />
        <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{filename}</span>
        {replacement && <span className="shrink-0">{count} {count === 1 ? "replacement" : "replacements"}</span>}
        <span className="min-w-0 flex-1 truncate text-[10.5px]">{replacement ? "Replacement preview" : operation.action === "append" ? "Append preview" : "Write preview"}{replacement?.unsupported ? " · partial" : ""}</span>
      </button>
    </FloatingTooltip>
    {opener}
    <button type="button" aria-label={`${open ? "Hide" : "Show"} file preview`} aria-expanded={open} aria-controls={regionId} onClick={toggle} className="flex shrink-0 items-center">
        <ChevronRight size={12} aria-hidden className={`shrink-0 ${open ? "rotate-90" : ""}`} />
    </button>
    </div>
    <div id={regionId} hidden={!open && !fullscreen}>
      {(open || fullscreen) && <BoundedMessageContent unbounded collapsedHeight="160px" data-testid="shell-replacement-fragments"
        className="mt-1" style={{ maxHeight: fullscreen ? "none" : "400px", overflow: "auto" }}>
        <div className="mb-1 text-[10.5px]">{replacement ? "Requested replacements" : operation.action === "append" ? "Content to append" : "Content to write"}</div>
        {operation.content !== undefined && <pre className="m-0 whitespace-pre-wrap break-words px-2 py-0.5" style={{ background: "var(--ct-diff-add-bg)", color: "var(--ct-fg)" }}>{operation.content}</pre>}
        {replacement?.fragments.map((fragment, index) => <div key={index} className="mb-1">
          <div className="text-[10.5px]">Before</div>
          <pre className="m-0 whitespace-pre-wrap break-words px-2 py-0.5" style={{ background: "var(--ct-diff-del-bg)", color: "var(--ct-fg)" }}>{fragment.before}</pre>
          <div className="text-[10.5px]">After</div>
          <pre className="m-0 whitespace-pre-wrap break-words px-2 py-0.5" style={{ background: "var(--ct-diff-add-bg)", color: "var(--ct-fg)" }}>{fragment.after}</pre>
        </div>)}
        {replacement?.unsupported && <div role="note" aria-label="Partial replacement preview">Some replacements cannot be previewed. See command details.</div>}
      </BoundedMessageContent>}
    </div>
  </div>}</ExpandableReader>;
}

export function shellFileOperationPreview(event: ToolEvent) {
  if ((event.kind !== "command" && event.kind !== "codeterm") || event.fileDiffs?.length || event.edits?.length || event.commandBatch?.length) return null;
  const input = event.message.toolInput;
  const hasCommand = [input?.command, input?.cmd, event.detail].some(value => typeof value === "string" && value.trim().length > 0);
  if (!hasCommand) return null;
  const { command, argv } = toolCommand(event);
  const analysis = shellFileOperations(command, argv);
  // An operation with no resolved path and nothing to preview, such as a redirect into `$DIR/file`, says nothing useful.
  const operations = analysis.operations.filter(operation => operation.path || operation.replacement || operation.content !== undefined);
  if (operations.length === 0) return null;
  return (
    <ul aria-label="Intended file operations" className="m-0 list-none px-3 py-0.5 font-mono text-[11.5px]" style={{ color: "var(--ct-muted-fg)" }}>
      {operations.map((operation, index) => (
        <li key={index} className="min-w-0">
          {operation.replacement || operation.content !== undefined ? <ReplacementPreview key={`${commandCallKey(event)}:${index}`} identity={`${commandCallKey(event)}:${index}`} operation={operation} /> : (
            <FloatingTooltip content={operation.path} disabled={!operation.path} className="block truncate">
              {operation.action === "append" ? "Intended append" : "Intended write"}{": "}{operation.path}
            </FloatingTooltip>
          )}
        </li>
      ))}
      {analysis.limited && <li>Preview limit reached</li>}
    </ul>
  );
}
