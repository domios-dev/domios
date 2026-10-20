import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { useComposerBackgroundWork } from "./useComposerBackgroundWork";

const state = { tabs: { tab: { id: "tab", agent_session_id: "root" } } } as unknown as ExtendedState;
const store = createMuxStateStore(() => state);

function messages(text: string): NormalizedChatMessage[] {
  return [{ id: "m1", type: "assistant", content: text }];
}

describe("composer background work", () => {
  it("keeps one value while transcript renders leave the work unchanged", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const values: Array<ReturnType<typeof useComposerBackgroundWork>> = [];
    function Probe({ transcript }: { transcript: NormalizedChatMessage[] }) {
      values.push(useComposerBackgroundWork("tab", false, true, transcript));
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    for (const transcript of [messages("one"), messages("one, streamed further")]) {
      await act(async () => root.render(<MuxStateStoreContext.Provider value={store}><Probe transcript={transcript} /></MuxStateStoreContext.Provider>));
    }
    expect(values[values.length - 1]).toBe(values[0]);
    await act(async () => root.unmount());
  });
});
