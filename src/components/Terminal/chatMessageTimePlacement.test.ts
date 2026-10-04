import { describe, expect, it } from "vitest";

import { agentMessageTimeIds } from "./chatMessageTimePlacement";

describe("agentMessageTimeIds", () => {
  it("keeps one assistant timestamp per user turn and resets at session changes", () => {
    const ids = agentMessageTimeIds([
      { id: "user-1", type: "user", content: "first question" },
      { id: "assistant-no-time", type: "assistant", content: "unstamped answer" },
      { id: "assistant-1", type: "assistant", content: "first answer", timestamp: "2024-01-01T13:05:00Z" },
      { id: "tool", type: "tool_call", content: "tool call" },
      { id: "assistant-2", type: "assistant", content: "answer continued", timestamp: "2024-01-01T13:06:00Z" },
      { id: "user-2", type: "user", content: "second question" },
      { id: "assistant-3", type: "assistant", content: "second answer", timestamp: "2024-01-01T13:07:00Z", sourceSessionId: "session-a" },
      { id: "assistant-4", type: "assistant", content: "second answer continued", timestamp: "2024-01-01T13:08:00Z", sourceSessionId: "session-a" },
      { id: "assistant-5", type: "assistant", content: "new session answer", timestamp: "2024-01-01T13:09:00Z", sourceSessionId: "session-b" },
      { id: "assistant-6", type: "assistant", content: "new session continuation", timestamp: "2024-01-01T13:10:00Z", sourceSessionId: "session-b" },
    ]);

    expect([...ids]).toEqual(["assistant-1", "assistant-3", "assistant-5"]);
  });
});
