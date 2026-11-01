import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { Tab, TabAgentState } from "../../lib/types";
import { TimelineMarkerRow } from "../../../packages/chat-ui/src/timelineMarkers";
import { useContextCompaction } from "./useContextCompaction";
import { projectCompactionRows } from "./contextCompactionModel";
import { compactionHistoryKey, contextCompactionStore } from "./contextCompactionStore";
import { conversationEntries } from "./chatPagingModel";
import { useContextCompactionObserver } from "./useContextCompactionObserver";

vi.mock("../../lib/frontendCacheRecords", () => ({
  frontendCacheScope: () => "fixture",
  frontendCacheRepository: { read: async () => null, write: async () => ({ refused: null }) },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("wires provider-neutral phases through the persisted model into a bottom row and one settled history marker", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const key = compactionHistoryKey("wiring", "session");
  const messages: NormalizedChatMessage[] = [{ id: "before", type: "assistant", content: "opaque" }];
  function Observer({ state }: { state: TabAgentState }) {
    useContextCompactionObserver({ wiring: { id: "wiring", agent_session_id: "session", agent_state: state } as Tab }, {}, true);
    return null;
  }
  function Surface({ state, rows = messages }: { state: TabAgentState; rows?: NormalizedChatMessage[] }) {
    const { history } = useContextCompaction("wiring", "session", rows);
    return <><Observer state={state} /><div role="log">{conversationEntries(projectCompactionRows(rows, history)).map(({ msg }) => <div key={msg.id} data-row={msg.id}>
      {msg.timelineMarker ? <TimelineMarkerRow message={msg} /> : <article />}
    </div>)}</div></>;
  }
  const render = async (state: TabAgentState, rows = messages) => {
    await act(async () => {
      root.render(<Surface state={state} rows={rows} />);
      await contextCompactionStore.hydrate(key);
    });
  };
  try {
    await act(async () => {
      root.render(<Observer state={{ state: "working", phase: "compacting" }} />);
      await contextCompactionStore.hydrate(key);
    });
    expect(contextCompactionStore.get(key).rows.length).toBe(1);
    const observedId = contextCompactionStore.get(key).rows[0].id;
    await render({ state: "working", phase: "compacting" });
    expect(host.querySelectorAll('[aria-busy="true"]').length).toBe(1);
    const markerId = host.querySelector('[role="log"]')?.lastElementChild?.getAttribute("data-row");
    expect(markerId).toBeTruthy();
    expect(markerId).toBe(observedId);
    const advanced = [...messages, { id: "during", type: "assistant" as const, content: "opaque later" }];
    await render({ state: "working", phase: "compacting" }, advanced);
    expect(host.querySelector('[role="log"]')?.lastElementChild?.getAttribute("data-row")).toBe(markerId);
    await render({ state: "working", phase: "streaming" }, advanced);
    const completed = contextCompactionStore.get(key);
    expect(completed.rows[0].state).toBe("completed");
    expect(host.querySelectorAll('[role="separator"]').length).toBe(1);
    expect(host.querySelector('[aria-busy="true"]')).toBeNull();
    const echoed = [...advanced, { id: "provider-completion", type: "system" as const, content: "", timelineMarker: { kind: "context_compaction" as const } }];
    await render({ state: "idle" }, echoed);
    expect(host.querySelectorAll('[role="separator"]').length).toBe(1);
    expect(host.querySelector('[role="log"]')?.lastElementChild?.getAttribute("data-row")).toBe(markerId);
    await render({ state: "working", phase: "compacting" }, echoed);
    await render({ state: "idle", phase: "failed" }, echoed);
    expect(contextCompactionStore.get(key).rows[1].state).toBe("failed");
    expect(host.querySelectorAll('[role="status"]').length).toBe(1);
  } finally { await act(async () => root.unmount()); }
});
