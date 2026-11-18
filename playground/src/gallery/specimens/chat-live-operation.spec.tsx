import { useState, type CSSProperties } from "react";
import { LocalArtifactScope, MessageBubble } from "@codeterm/chat-ui";
import { formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { ChatBottomBar } from "../ChatBottomBar";
import { agentTab } from "../fixtures";
import { defineSpecimen } from "../types";
import { mockLocalPreview } from "../../mocks/backend";

const operation: NormalizedChatMessage = {
  id: "fixture-live-operation", type: "tool_call", content: "", toolKind: "command", toolStatus: "running",
  toolInput: { cmd: "domios plugin browser goto http://127.0.0.1:14997/;\ndomios plugin browser snapshot --with-text --max-items 80;\ndomios plugin browser screenshot evidence/chat-running-dark.png --full" },
  toolStartedAtMs: Date.now() - 12_000,
};
const tab = agentTab("fixture-live-chat", { title: "Pulse", agent_state: { state: "working" } } as Parameters<typeof agentTab>[1]);
const layoutNotes = "/tmp/gallery/layout-notes.txt";
mockLocalPreview(layoutNotes, { data: "Compare chat layout in light and dark.", media_type: "text/plain" });

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
        <LocalArtifactScope tabId={tab.id}>
          <MessageBubble message={{ id: "fixture-user", type: "user", content: `Check the chat layout in both themes. ${formatAttachmentMarker(layoutNotes, "file", "layout-notes.txt")}` }} />
        </LocalArtifactScope>
        <MessageBubble message={{ id: "fixture-assistant", type: "assistant", content: "I’m inspecting the chat fixture and capturing the current operation." }} />
        <MessageBubble message={operation} />
      </div>
      <ChatBottomBar tab={tab} />
    </div>
  </div>;
}

export default defineSpecimen({
  id: "chat-live-operation", title: "Chat live operation", category: "Chat", stage: "wide",
  where: "packages/chat-ui/src/MessageBubble.tsx", Component: LiveChatStage,
  states: [{ id: "running", label: "Running operation · transcript tail", props: {} }],
});
