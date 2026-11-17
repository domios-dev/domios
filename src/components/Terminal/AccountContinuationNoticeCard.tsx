import { useState } from "react";
import { ChatNoticeCard } from "@codeterm/chat-ui";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

export function AccountContinuationNoticeCard({message}:{message:NormalizedChatMessage}) {
  const [expanded,setExpanded]=useState(false);
  const resets=typeof message.toolInput?.resetsAtMs==="number" ? new Date(message.toolInput.resetsAtMs).toLocaleString():null;
  return <ChatNoticeCard testId="account-continued-notice" quiet role="status" icon={null} title={message.content} expanded={expanded} onToggle={()=>setExpanded(value=>!value)}>
    {resets && <p>Limit resets {resets}.</p>}
    {typeof message.toolInput?.boundary==="string" && <p>Boundary: {message.toolInput.boundary}</p>}
  </ChatNoticeCard>;
}
