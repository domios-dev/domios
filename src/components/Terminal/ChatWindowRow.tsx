import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { hasChatWindowRow } from "./chatWindowRowsModel";

export function ChatWindowRow({ message, children, liveActivityLabel }: {
  message: NormalizedChatMessage;
  children: ReactNode;
  liveActivityLabel?: string;
}) {
  if (!hasChatWindowRow(message)) return null;
  return <div
    data-chat-window-row={message.id}
    role={liveActivityLabel ? "status" : undefined}
    aria-live={liveActivityLabel ? "polite" : undefined}
    aria-label={liveActivityLabel}
    style={{ display: "flow-root" }}
  >{children}</div>;
}
