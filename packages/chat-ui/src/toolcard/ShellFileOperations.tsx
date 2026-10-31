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
  if (!replacement) return null;
  const count = replacement.fragments.length;
  const filename = operation.path?.split(/[\\/]/).pop() || operation.path;
  return <>
    <FloatingTooltip content={operation.path} focusable={false} className="block min-w-0">
      <button type="button" aria-expanded={open} aria-controls={regionId}
        aria-label={`${open ? "Hide" : "Show"} replacement preview for ${filename}`}
        className="flex w-full min-w-0 items-center gap-1.5 text-left"
        onClick={() => {
          const next = !open;
          if (identity.length <= 2_048) {
            if (OPEN_PREVIEWS.size >= 64) OPEN_PREVIEWS.delete(OPEN_PREVIEWS.keys().next().value as string);
            OPEN_PREVIEWS.set(identity, next);
          }
          setOpen(next);
        }}>
        <FileDiff size={14} aria-hidden className="shrink-0" />
        <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{filename}</span>
        <span className="shrink-0">{count} {count === 1 ? "replacement" : "replacements"}</span>
        <span className="min-w-0 flex-1 truncate text-[10.5px]">Replacement preview{replacement.unsupported ? " · partial" : ""}</span>
        <ChevronRight size={12} aria-hidden className={`shrink-0 ${open ? "rotate-90" : ""}`} />
      </button>
    </FloatingTooltip>
    <div id={regionId} hidden={!open}>
      {open && <BoundedMessageContent unbounded collapsedHeight="160px" data-testid="shell-replacement-fragments"
        className="mt-1" style={{ maxHeight: "400px", overflow: "auto" }}>
        <div className="mb-1 text-[10.5px]">Requested replacements</div>
        {replacement.fragments.map((fragment, index) => <div key={index} className="mb-1">
          <div className="text-[10.5px]">Before</div>
          <pre className="m-0 whitespace-pre-wrap break-words px-2 py-0.5" style={{ background: "var(--ct-diff-del-bg)", color: "var(--ct-fg)" }}>{fragment.before}</pre>
          <div className="text-[10.5px]">After</div>
          <pre className="m-0 whitespace-pre-wrap break-words px-2 py-0.5" style={{ background: "var(--ct-diff-add-bg)", color: "var(--ct-fg)" }}>{fragment.after}</pre>
        </div>)}
        {replacement.unsupported && <div role="note" aria-label="Partial replacement preview">Some replacements cannot be previewed. See command details.</div>}
      </BoundedMessageContent>}
    </div>
  </>;
}

export function shellFileOperationPreview(event: ToolEvent) {
  if ((event.kind !== "command" && event.kind !== "codeterm") || event.fileDiffs?.length || event.edits?.length || event.commandBatch?.length) return null;
  const input = event.message.toolInput;
  const hasCommand = [input?.command, input?.cmd, event.detail].some(value => typeof value === "string" && value.trim().length > 0);
  if (!hasCommand) return null;
  const { command, argv } = toolCommand(event);
  const analysis = shellFileOperations(command, argv);
  if (analysis.operations.length === 0) return null;
  return (
    <ul aria-label="Intended file operations" className="m-0 list-none px-3 py-0.5 font-mono text-[11.5px]" style={{ color: "var(--ct-muted-fg)" }}>
      {analysis.operations.map((operation, index) => (
        <li key={index} className="min-w-0">
          {operation.replacement ? <ReplacementPreview key={`${commandCallKey(event)}:${index}`} identity={`${commandCallKey(event)}:${index}`} operation={operation} /> : (
            <FloatingTooltip content={operation.path} disabled={!operation.path} className="block truncate">
              {operation.action === "append" ? "Intended append" : "Intended write"}{": "}{operation.path ?? "unresolved target"}
            </FloatingTooltip>
          )}
        </li>
      ))}
      {analysis.limited && <li>Preview limit reached</li>}
    </ul>
  );
}
