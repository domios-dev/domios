import { toolCommandLabel, type ToolEvent } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";
import { shellFileOperationPreview } from "./ShellFileOperations";
import { toolCommandShape } from "../codeterm/commandShape";
import { LiveCommandOutput, liveOutputTail } from "./LiveCommandOutput";

function resultTail(result: string | undefined): string | undefined {
  const lines = result ? liveOutputTail(result.trimEnd()).split("\n").map((line) => line.trim()).filter(Boolean) : [];
  return lines[lines.length - 1];
}

export function CommandCard({ event, expanded, onToggle, children, primaryContent }: { event: ToolEvent; expanded: boolean; onToggle: () => void; children?: ReactNode; primaryContent?: ReactNode }) {
  const tail = event.status !== "pending" ? resultTail(event.result) : undefined;
  const preview = shellFileOperationPreview(event);
  const expandable = event.status === "pending" || Boolean(event.result) || Boolean(event.fileDiffs?.length) || toolCommandShape(event).kind !== "single";
  return <ToolCardFrame event={event} expandable={expandable} minHeight={36} primaryContent={preview ? <>{primaryContent}{preview}</> : primaryContent} liveOutput={<LiveCommandOutput event={event} />} expanded={expanded} onToggle={onToggle} title={event.title || toolKindVisual[event.kind].label} summary={event.description || toolCommandLabel(event)} disclosureLabel="full command" tail={tail && <span data-testid="toolcard-result-tail" className="max-w-[34%] truncate font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>→ {tail}</span>}>
    {children ?? (
      <>
        {event.status === "pending" && (
          <div data-testid="toolcard-spinner" className="flex items-center gap-1.5 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
            <span aria-hidden="true" className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-accent)" }} />
            <span>Running…</span>
          </div>
        )}
        {event.result && (
          <div data-testid="toolcard-result" className={event.status === "pending" ? "mt-1.5" : undefined}>
            <ToolOutputContent text={event.result} maxHeight="12rem" />
          </div>
        )}
      </>
    )}
  </ToolCardFrame>;
}
