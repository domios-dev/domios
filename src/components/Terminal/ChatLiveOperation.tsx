import { useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ToolCallProgress } from "../../../packages/chat-ui/src/toolcard/ToolCallProgress";
import { toolKindVisual } from "../../../packages/chat-ui/src/toolcard/toolKindVisual";
import { BusyState } from "../shared/BusyState";
import { chatLiveOperationFullLabel } from "./chatTranscriptActivity";

export function ChatLiveOperation({ message }: { message: NormalizedChatMessage | null }) {
  if (!message || message.toolStatus !== "running") return null;
  return <LiveOperation key={message.id} message={message} />;
}

function LiveOperation({ message }: { message: NormalizedChatMessage }) {
  const [expanded, setExpanded] = useState(false);
  const visual = toolKindVisual[message.toolKind ?? "generic"];
  const Icon = visual.glyph;
  const label = chatLiveOperationFullLabel(message);
  return <section
    data-testid="chat-live-operation"
    aria-label="Live operation"
    className="my-2 rounded-lg border p-3 text-xs"
    style={{ borderColor: "var(--ct-border-default)", background: "var(--ct-surface)", color: "var(--ct-fg)" }}
  >
    <div className="mb-2 flex items-center gap-2 text-[var(--ct-muted-fg)]">
      <Icon size={14} style={{ color: visual.color }} aria-hidden />
      <span>{visual.label}</span>
      <BusyState inline hideLabel label="Operation running" phase="work" dotSize="micro" />
      <span className="ml-auto tabular-nums"><ToolCallProgress message={message} /></span>
    </div>
    <div id={`live-operation-${message.id}`} className="whitespace-pre-wrap break-words font-mono leading-5"
      style={{ height: expanded ? "12rem" : "4rem", overflowY: expanded ? "auto" : "hidden", overflowWrap: "anywhere" }}
    >{label}</div>
    <button type="button" aria-expanded={expanded} aria-controls={`live-operation-${message.id}`}
      onClick={() => setExpanded(!expanded)} className="mt-1 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
    >{expanded ? "Show less" : "Show full operation"}</button>
  </section>;
}
