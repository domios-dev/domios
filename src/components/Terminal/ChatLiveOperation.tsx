import { useState } from "react";
import { materializeToolEvent, type NormalizedChatMessage, type ToolEvent } from "@codeterm/chat-core";
import { toolCommandShape } from "../../../packages/chat-ui/src/codeterm/commandShape";
import { ToolOutputContent } from "../../../packages/chat-ui/src/ToolOutputContent";
import { CommandCard } from "../../../packages/chat-ui/src/toolcard/CommandCard";
import { CommandGroupCard } from "../../../packages/chat-ui/src/toolcard/CommandGroupCard";
import { commandCallKey } from "../../../packages/chat-ui/src/toolcard/CommandTree";
import { ToolCardFrame } from "../../../packages/chat-ui/src/toolcard/ToolCardFrame";
import { chatLiveOperationFullLabel } from "./chatTranscriptActivity";

export function ChatLiveOperation({ message }: { message: NormalizedChatMessage | null }) {
  if (!message || message.toolStatus !== "running") return null;
  const event = message.toolKind === "command" ? materializeToolEvent(message) : null;
  if (event) return <LiveCommand key={commandCallKey(event)} event={event} />;
  return <LiveOperation key={message.id} message={message} />;
}

function LiveCommand({ event }: { event: ToolEvent }) {
  const [expanded, setExpanded] = useState(false);
  const shape = toolCommandShape(event);
  return <section data-testid="chat-live-operation" aria-label="Live operation">
    {shape.kind === "group"
      ? <CommandGroupCard event={event} rows={shape.rows} expanded={expanded} onToggle={setExpanded} />
      : <CommandCard event={event} expanded={expanded} onToggle={() => setExpanded(!expanded)}>
        {event.result ? <ToolOutputContent text={event.result} maxHeight="12rem" /> : undefined}
      </CommandCard>}
  </section>;
}

function LiveOperation({ message }: { message: NormalizedChatMessage }) {
  const [expanded, setExpanded] = useState(false);
  const event = materializeToolEvent(message);
  if (!event) return null;
  const label = chatLiveOperationFullLabel(message);
  return <section data-testid="chat-live-operation" aria-label="Live operation">
    <ToolCardFrame event={event} expanded={expanded} onToggle={() => setExpanded(!expanded)}
      summary={label} disclosureLabel="full operation"
    >
      <div className="whitespace-pre-wrap break-words font-mono text-xs leading-5" style={{ overflowWrap: "anywhere" }}>{label}</div>
      {event.result && <ToolOutputContent text={event.result} maxHeight="12rem" />}
    </ToolCardFrame>
  </section>;
}
