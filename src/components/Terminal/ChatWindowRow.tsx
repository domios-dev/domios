import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { hasChatWindowRow } from "./chatWindowRowsModel";

export function ChatWindowRow({ message, children }: { message: NormalizedChatMessage; children: ReactNode }) {
  if (!hasChatWindowRow(message)) return null;
  return <div data-chat-window-row={message.id} style={{ display: "flow-root" }}>{children}</div>;
}
