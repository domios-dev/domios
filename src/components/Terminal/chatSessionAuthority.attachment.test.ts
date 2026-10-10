import { describe, expect, it } from "vitest";
import { attachmentActivity, attachmentStateForSnapshot, type ChatStartResult } from "./chatSessionAuthority";

describe("bound file-tail attachment", () => {
  it.each([
    { messages: [] },
    { messages: [{ id: "row", type: "assistant" as const, content: "answer", timestamp: "1" }] },
  ])(
    "completes a non-pending snapshot independently of row count: %j",
    ({ messages }) => {
      const snapshot: ChatStartResult = {
        source_session_id: "own-session",
        session_binding_nonce: "own-binding",
        text: "",
        offset: 0,
        pending: false,
        messages,
      };
      expect(attachmentActivity("awaiting_store", false, true).recovering).toBe(true);
      const state = attachmentStateForSnapshot(snapshot);
      expect(state).toBe("attached");
      expect(attachmentActivity(state, true, true)).toEqual({ loading: false, recovering: false });
    },
  );

  it("keeps a pending store in recovery until observed readiness", () => {
    const state = attachmentStateForSnapshot({ pending: true });
    expect(state).toBe("awaiting_store");
    expect(attachmentActivity(state, false, true)).toEqual({ loading: false, recovering: true });
    expect(attachmentActivity("attached", false, true)).toEqual({ loading: false, recovering: false });
  });
});
