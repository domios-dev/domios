// Run: npx tsx src/components/Terminal/useChatSession.optimisticIdentity.test.ts
// addOptimistic reaches memoized chat rows as a prop, so streaming must not
// churn its identity — while the arrival ordinal still comes from the live
// transcript rather than one captured when the callback was created.
import type { NormalizedChatMessage } from "@codeterm/chat-core";
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
}

async function main(): Promise<void> {
  setupHookDom();
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");

  const listeners = new Map<string, Set<Handler>>();
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
    view = { addOptimistic: session.addOptimistic, allMessages: session.allMessages };
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
  let firstId = "";
  await act(async () => {
    firstId = v().addOptimistic("first");
  });
  await flush();
  const firstOrder = v().allMessages.find((row) => row.id === firstId)?.clientOrder;
  assert(typeof firstOrder === "number", "optimistic row carries an arrival ordinal");

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

  let secondId = "";
  await act(async () => {
    secondId = v().addOptimistic("second");
  });
  await flush();
  const secondOrder = v().allMessages.find((row) => row.id === secondId)?.clientOrder;
  assert(typeof secondOrder === "number", "second optimistic row carries an ordinal");
  assert(
    secondOrder > firstOrder,
    `the ordinal reads the live transcript, not one captured at creation (${firstOrder} -> ${secondOrder})`,
  );

  await act(async () => root.unmount());
  host.remove();
  console.log("optimistic identity + ordering checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
