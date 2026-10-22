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
  it("subscribes to PTY provider roster changes without transcript or runtime support", async () => {
    let current = { tabs: { tab: { id: "tab", agent_session_id: "own" } } } as unknown as ExtendedState;
    const mux = createMuxStateStore(() => current);
    let work!: ReturnType<typeof useComposerBackgroundWork>;
    function Probe() { work = useComposerBackgroundWork("tab", false, false, []); return null; }
    const root = createRoot(document.createElement("div"));
    try {
      await act(async () => root.render(<MuxStateStoreContext.Provider value={mux}><Probe /></MuxStateStoreContext.Provider>));
      expect(work.items).toHaveLength(0);
      current = { ...current, tabs: { tab: { ...current.tabs.tab, provider_signals: { auth: null, api_error: null, permission_mode: null, background: { running: { child: "Inspect source" }, overflow: false, history_incomplete: false, history_initialized: true } } } } };
      await act(async () => mux.notify());
      expect(work.items.map(item => item.threadId)).toEqual(["child"]);
      current = { ...current, tabs: { tab: { ...current.tabs.tab, provider_signals: undefined } } };
      await act(async () => mux.notify());
      expect(work.items).toHaveLength(0);
    } finally { await act(async () => root.unmount()); }
  });
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
