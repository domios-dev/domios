import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "./bridge";
import { setAgentDescriptors } from "./agentDescriptorCache";
import { setCachedShellModel } from "./shellSessionModel";
import { useShellSessionModel } from "./useTabProvider";
import type { ProviderDescriptor, Tab } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function shellTab(pluginId: string, sessionId: string): Tab {
  return {
    id: `tab-${sessionId}`,
    title: "Shell",
    backing: { kind: "chat_backend", plugin_id: pluginId, session_id: sessionId },
  } as Tab;
}

function shellDescriptor(id: string): ProviderDescriptor {
  return {
    id,
    display_name: "Opaque Shell",
    available_models: [],
  } as unknown as ProviderDescriptor;
}

function Probe({ tab }: { tab: Tab }) {
  const model = useShellSessionModel(tab);
  return <span data-testid="shell-model">{model ?? "none"}</span>;
}

afterEach(() => {
  __setInvokeForTest(null);
  __setListenForTest(null);
  setAgentDescriptors([]);
});

it("fetches once per session without subscribing to mux-state-changed and reflects direct switches", async () => {
  const pluginId = "opaque-shell-provider";
  const sessionId = "session-no-mux-refetch";
  setAgentDescriptors([shellDescriptor(pluginId)]);

  const listenedEvents: string[] = [];
  let sessionInfoCalls = 0;
  __setListenForTest(async (event) => {
    listenedEvents.push(event);
    return () => {};
  });
  __setInvokeForTest(async <T,>(command: string) => {
    if (command === "chatbackend_session_info") {
      sessionInfoCalls += 1;
      return { model: "opaque-model-1" } as T;
    }
    throw new Error(`unexpected command: ${command}`);
  });

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(<Probe tab={shellTab(pluginId, sessionId)} />);
    });
    expect(sessionInfoCalls).toBe(1);
    expect(listenedEvents).not.toContain("mux-state-changed");
    expect(host.querySelector('[data-testid="shell-model"]')?.textContent).toBe("opaque-model-1");

    await act(async () => {
      setCachedShellModel(pluginId, sessionId, "opaque-model-2");
    });
    expect(host.querySelector('[data-testid="shell-model"]')?.textContent).toBe("opaque-model-2");
    expect(sessionInfoCalls).toBe(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
