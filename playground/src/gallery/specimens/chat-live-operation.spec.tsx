import { useState, type CSSProperties } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ChatLiveOperation } from "../../../../src/components/Terminal/ChatLiveOperation";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { ChatBottomBar } from "../ChatBottomBar";
import { agentTab } from "../fixtures";
import { defineSpecimen } from "../types";

const operation: NormalizedChatMessage = {
  id: "fixture-live-operation", type: "tool_call", content: "", toolKind: "command", toolStatus: "running",
  toolInput: { cmd: "domios plugin browser goto http://127.0.0.1:14997/;\ndomios plugin browser snapshot --with-text --max-items 80;\ndomios plugin browser screenshot evidence/chat-running-dark.png --full" },
  toolStartedAtMs: Date.now() - 12_000,
};
const tab = agentTab("fixture-live-chat", { title: "Pulse", agent_state: { state: "working" } } as Parameters<typeof agentTab>[1]);

function LiveChatStage() {
  const [light, setLight] = useState(false);
  const theme = light ? { ...DEFAULT_THEME, background: "#ffffff", foreground: "#24292f", bright_black: "#57606a", blue: "#0969da" } : DEFAULT_THEME;
  return <div style={{ ...themeToCSS(theme), color: "var(--ct-fg)", background: "var(--ct-bg)", padding: 20 } as CSSProperties}>
    <div className="mb-3 flex gap-3 text-xs">
      <button type="button" onClick={() => setLight(true)}>Light theme</button>
      <button type="button" onClick={() => setLight(false)}>Dark theme</button>
    </div>
    <div className="flex flex-col" style={{ height: 460 }}>
      <div role="log" aria-label="Conversation" className="min-h-0 flex-1 overflow-y-auto">
        <MessageBubble message={{ id: "fixture-user", type: "user", content: "Check the chat layout in both themes." }} />
        <MessageBubble message={{ id: "fixture-assistant", type: "assistant", content: "I’m inspecting the chat fixture and capturing the current operation." }} />
        <ChatLiveOperation message={operation} />
      </div>
      <ChatBottomBar tab={tab} />
    </div>
  </div>;
}

export default defineSpecimen({
  id: "chat-live-operation", title: "Chat live operation", category: "Chat", stage: "wide",
  where: "src/components/Terminal/ChatLiveOperation.tsx", Component: LiveChatStage,
  states: [{ id: "running", label: "Running operation · transcript tail", props: {} }],
});
