import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  sessionBindingSurfaceFromFact,
  type SessionBindingFact,
} from "./sessionBindingSurface";

type Test = [string, () => void | Promise<void>];

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function fact(
  kind: SessionBindingFact["kind"],
  overrides: Partial<SessionBindingFact> = {},
): SessionBindingFact {
  return {
    tab_id: "tab-a",
    kind,
    current_session_id: "session-a",
    requested_session_id: null,
    reason: null,
    ...overrides,
  };
}

const tests: Test[] = [
  ["binding_state_surface is driven directly by the daemon binding fact", () => {
    const surface = sessionBindingSurfaceFromFact(
      "tab-a",
      "session-a",
      fact("refused", { requested_session_id: "requested" }),
    );
    assert(surface.agentState?.state === "quiet", "a refused daemon fact reaches the quiet rail");
    assert(surface.showRecoveryAction, "the refusal exposes recovery without a status sequence");
  }],
  ["binding_state_surface gives all five daemon states their stated outcome", () => {
    const deferred = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("deferred_pending"));
    const refused = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("refused"));
    const unbound = sessionBindingSurfaceFromFact(
      "tab-a",
      null,
      fact("unbound", { current_session_id: null }),
    );
    const healthy = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("healthy"));
    const stale = sessionBindingSurfaceFromFact("tab-a", "replacement", fact("stale_pending", {
      current_session_id: "replacement",
      requested_session_id: "abandoned",
    }));
    assert(deferred.agentState?.state === "quiet" && !deferred.showRecoveryAction, "pending is recovering without demanding action");
    assert(refused.agentState?.state === "quiet" && refused.showRecoveryAction, "refused needs recovery");
    assert(unbound.agentState?.state === "quiet" && unbound.showRecoveryAction, "live unbound needs recovery");
    assert(healthy.agentState === null, "healthy is silent");
    assert(stale.agentState === null, "stale pending never announces recovery");
  }],
  ["binding_state_surface keeps the common healthy tab silent", () => {
    const surface = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("healthy"));
    assert(surface.agentState === null, "no registry fact plus the valid own session is silent");
    assert(!surface.showRecoveryAction, "healthy tabs expose no repair action");
  }],
  ["binding_state_surface isolates a same-cwd sibling tab in both directions", () => {
    const siblingRefusal = fact("refused", {
      tab_id: "tab-b",
      current_session_id: "session-b",
      requested_session_id: "requested-b",
    });
    const first = sessionBindingSurfaceFromFact("tab-a", "session-a", siblingRefusal);
    const second = sessionBindingSurfaceFromFact("tab-b", "session-b", fact("healthy", {
      tab_id: "tab-a",
    }));
    assert(first.agentState === null, "the sibling refusal cannot contaminate this healthy tab");
    assert(second.agentState === null, "this tab's healthy fact cannot clear a sibling verdict");
  }],
  ["binding_state_surface handles the live agent with no registry entry or session id", () => {
    const surface = sessionBindingSurfaceFromFact(
      "tab-a",
      null,
      fact("unbound", { current_session_id: null }),
    );
    assert(surface.agentState?.state === "quiet", "the #291 route does not fall through as healthy");
    assert(surface.showRecoveryAction, "the unbound tab offers the recovery mechanism");
  }],
  ["binding_state_surface reaches rebind through the visible repair mechanism", async () => {
    const window = new Window({ url: "http://localhost:7686/" });
    Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
    Object.defineProperty(globalThis, "window", { configurable: true, value: window });
    Object.defineProperty(globalThis, "document", { configurable: true, value: window.document });
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: window.navigator });
    Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: window.HTMLElement });
    Object.defineProperty(globalThis, "SVGElement", { configurable: true, value: window.SVGElement });
    Object.defineProperty(globalThis, "Event", { configurable: true, value: window.Event });
    const { __setInvokeForTest } = await import("../../lib/bridge");
    const { AgentStateIndicator } = await import("./AgentStateIndicator");
    const { SessionBindingRecoveryForm } = await import("./SessionBindingRecoveryForm");
    const commands: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invokeMock = async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
      commands.push({ command, args });
      return {} as T;
    };
    __setInvokeForTest(invokeMock);

    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <AgentStateIndicator state={{ state: "quiet" }} onRebind={() => setOpen(true)} />
          {open && (
            <SessionBindingRecoveryForm
              tabId="tab-a"
              suggestedSessionId="requested-session"
              onClose={() => setOpen(false)}
            />
          )}
        </>
      );
    }

    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Harness />));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="agent-rebind"]')?.click());
    const form = host.querySelector<HTMLFormElement>('[data-testid="session-binding-recovery"]');
    assert(form, "the rail action opens the in-tab recovery form");
    const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]');
    assert(submit, "the recovery form exposes its submit action in focus order");
    await act(async () => {
      submit.click();
      await Promise.resolve();
    });
    assert(commands.some(({ command, args }) => command === "rebind_tab_session"
      && args?.tabId === "tab-a"
      && args?.sessionId === "requested-session"), "the form invokes the tab rebind mechanism");
    await act(async () => root.unmount());
    __setInvokeForTest(null);
  }],
];

for (const [name, run] of tests) {
  try {
    await run();
    console.log(`PASS ${name}`);
  } catch (error) {
    console.error(`FAIL ${name}`);
    throw error;
  }
}
