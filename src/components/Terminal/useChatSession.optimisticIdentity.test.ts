// Run: npx tsx src/components/Terminal/useChatSession.optimisticIdentity.test.ts
// addOptimistic reaches memoized rows: identity survives streaming, the send key is captured once.
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function setupHookDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Event", "CustomEvent"] as const) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (hookWindow as never)[key] });
  }
}

function msg(id: string, content: string, seq: number): NormalizedChatMessage {
  return { id, type: "assistant", content, timestamp: String(1_700_000_000_000 + seq * 1000), seq };
}

type Handler = (event: { payload: unknown }) => void;

interface HookView {
  addOptimistic: (content: string) => string;
  allMessages: NormalizedChatMessage[];
  observeChatSendStatus: (status: ChatSendStatus) => void;
}

async function main(): Promise<void> {
  setupHookDom();
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");

  const listeners = new Map<string, Set<Handler>>();
  let confirmedStatuses: ChatSendStatus[] = [];
  (bridge as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(
    async (event: string, handler: Handler) => {
      let set = listeners.get(event);
      if (!set) {
        set = new Set();
        listeners.set(event, set);
      }
      set.add(handler);
      return () => {
        set?.delete(handler);
      };
    },
  );
  (bridge as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(
    async (command: string) => {
      if (command === "chat_start") {
        return {
          text: "",
          start_offset: 0,
          offset: 0,
          pending: false,
          messages: [msg("a-0", "hello", 0)],
          source_session_id: "sid-1",
          session_binding_nonce: "nonce-1",
        };
      }
      if (command === "chat_send_status") return confirmedStatuses;
      return {};
    },
  );

  let view: HookView | null = null;
  const v = (): HookView => {
    if (!view) throw new Error("hook view not rendered");
    return view;
  };
  function Harness() {
    const session = useChatSession({
      tabId: "tab",
      agentSessionId: "sid-1",
      sessionBindingNonce: "nonce-1",
      active: true,
    });
    view = {
      addOptimistic: session.addOptimistic,
      allMessages: session.allMessages,
      observeChatSendStatus: session.observeChatSendStatus,
    };
    return null;
  }
  const flush = async (): Promise<void> => {
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
  };

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(createElement(Harness));
  });
  await flush();

  const mountedIdentity = v().addOptimistic;
  let chipToken = "";
  await act(async () => {
    chipToken = v().addOptimistic("see task_x");
    v().observeChatSendStatus({
      send_id: "send-chip",
      tab_id: "tab",
      state: "accepted",
      error: null,
      ledger_id: "ledger",
      sequence: 1,
      retained_from: 1,
      correlationId: chipToken,
    });
  });
  confirmedStatuses = [{
    send_id: "send-chip",
    tab_id: "tab",
    state: "confirmed",
    error: null,
    ledger_id: "ledger",
    sequence: 1,
    retained_from: 1,
    correlationId: chipToken,
  }];
  await act(async () => {
    for (const handler of listeners.get("tab-chat-msg-tab") ?? []) {
      handler({
        payload: {
          tab_id: "tab",
          source_session_id: "sid-1",
          session_binding_nonce: "nonce-1",
          messages: [{
            id: "chip-echo",
            type: "user",
            content: "◆ task_x — enriched chip label ◆",
            timestamp: String(Date.now()),
          }],
        },
      });
    }
    await Promise.resolve();
  });
  await flush();
  const chipRows = v().allMessages.filter((row) => row.type === "user");
  assert(chipRows.length === 1, "chip-divergent optimistic and committed turns render one bubble");
  assert(chipRows[0]?.id === "chip-echo", "the committed chip-bearing row wins by correlation id");

  let firstId = "";
  await act(async () => {
    firstId = v().addOptimistic("first");
  });
  await flush();
  const firstKey = v().allMessages.find((row) => row.id === firstId)?.timestamp;
  assert(typeof firstKey === "string", "a pending send projects with its captured send key");

  await act(async () => {
    for (const handler of listeners.get("tab-chat-msg-tab") ?? []) {
      handler({
        payload: {
          tab_id: "tab",
          source_session_id: "sid-1",
          session_binding_nonce: "nonce-1",
          messages: [msg("a-1", "streamed", 1)],
        },
      });
    }
  });
  await flush();
  assert(
    v().allMessages.some((row) => row.content === "streamed"),
    "the streamed delta landed (ordinal assertions below depend on it)",
  );

  assert(
    v().addOptimistic === mountedIdentity,
    "addOptimistic keeps identity across streamed transcript updates",
  );
  assert(
    v().allMessages.find((row) => row.id === firstId)?.timestamp === firstKey,
    "streamed output never rewrites a pending send's key",
  );

  let secondId = "";
  await act(async () => {
    secondId = v().addOptimistic("second");
  });
  await flush();
  const rows = v().allMessages;
  const secondKey = rows.find((row) => row.id === secondId)?.timestamp;
  assert(typeof secondKey === "string", "the second send projects with its own key");
  assert(
    Number(secondKey) >= Number(firstKey),
    `a later send never takes an earlier key (${firstKey} -> ${secondKey})`,
  );
  assert(
    rows.findIndex((row) => row.id === firstId) < rows.findIndex((row) => row.id === secondId),
    "two pending sends project in send order",
  );

  await act(async () => root.unmount());
  host.remove();
  console.log("optimistic identity + ordering checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
