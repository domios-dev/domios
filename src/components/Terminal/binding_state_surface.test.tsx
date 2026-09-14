import { Component, act, useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
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

function unknownKindFact(): SessionBindingFact {
  return {
    ...fact("healthy", { current_session_id: null }),
    kind: "kind_this_build_does_not_map" as SessionBindingFact["kind"],
  };
}

function installDom(): void {
  const window = new Window({ url: "http://localhost:7686/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  Object.defineProperty(globalThis, "window", { configurable: true, value: window });
  Object.defineProperty(globalThis, "document", { configurable: true, value: window.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: window.navigator });
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: window.HTMLElement });
  Object.defineProperty(globalThis, "SVGElement", { configurable: true, value: window.SVGElement });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: window.Event });
}

class RenderGuard extends Component<{ children: ReactNode }, { tripped: boolean }> {
  state = { tripped: false };
  static getDerivedStateFromError(): { tripped: boolean } {
    return { tripped: true };
  }
  render(): ReactNode {
    if (this.state.tripped) return <div data-testid="render-guard-tripped" />;
    return this.props.children;
  }
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
  ["binding_state_surface gives all daemon states their stated outcome", () => {
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
  ["binding_state_surface_silences_never_bound_launch", () => {
    const surface = sessionBindingSurfaceFromFact(
      "tab-a",
      null,
      fact("never_bound", { current_session_id: null }),
    );
    assert(surface.agentState === null, "a never-bound launch stays silent");
    assert(!surface.showRecoveryAction, "a never-bound launch exposes no repair action");
  }],
  ["binding_state_surface_keeps_recovery_for_lost_binding", () => {
    const surface = sessionBindingSurfaceFromFact(
      "tab-a",
      null,
      fact("unbound", { current_session_id: null }),
    );
    assert(surface.agentState?.status_line === "Session not connected", "a lost binding exposes the required status");
    assert(surface.showRecoveryAction, "a lost binding keeps the repair action");
  }],
  ["binding_state_surface silences a fact carrying a foreign current session", () => {
    const foreign = sessionBindingSurfaceFromFact(
      "tab-a",
      "session-a",
      fact("deferred_pending", { current_session_id: "session-foreign" }),
    );
    assert(foreign.agentState === null, "a fact about another session cannot speak for this tab");
    assert(!foreign.showRecoveryAction, "the foreign fact offers no action either");
    const own = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("deferred_pending"));
    assert(own.agentState?.state === "quiet", "the guard still lets this tab's own fact through");
  }],
  ["binding_state_surface stays non-authoritative on a deferred fact", () => {
    const deferred = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("deferred_pending"));
    const healthy = sessionBindingSurfaceFromFact("tab-a", "session-a", fact("healthy"));
    assert(healthy.agentState === null, "healthy is the only silent verdict of the two");
    assert(
      deferred.agentState?.state === "quiet"
        && typeof deferred.agentState.status_line === "string"
        && deferred.agentState.status_line.length > 0
        && !deferred.showRecoveryAction,
      "a backend deferral keeps its own quiet line and is never optimistically cleared",
    );
  }],
  ["binding_state_surface_maps_an_unknown_daemon_kind_to_a_silent_surface", () => {
    const surface = sessionBindingSurfaceFromFact("tab-a", null, unknownKindFact());
    assert(surface != null, "a kind this build does not map still yields a surface object");
    assert(surface.agentState === null, "an unmapped kind stays silent");
    assert(surface.showRecoveryAction === false, "an unmapped kind offers no repair action");
    assert(surface.rebindSessionId === null, "an unmapped kind suggests no rebind target");
  }],
  ["binding_state_surface_renders_the_empty_state_for_an_unknown_daemon_kind", async () => {
    installDom();
    const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
    const { useSessionBindingSurface } = await import("./useSessionBindingSurface");

    let deliver: ((fact: SessionBindingFact) => void) | null = null;
    let signalSubscribed: () => void = () => undefined;
    const subscribed = new Promise<void>((resolve) => {
      signalSubscribed = resolve;
    });
    __setListenForTest(async (event, handler) => {
      if (event === "session-binding-state-changed") {
        deliver = (fact) => handler({ payload: fact as never });
        signalSubscribed();
      }
      return () => undefined;
    });
    __setInvokeForTest(async () => {
      throw new Error("no snapshot read in this test");
    });

    // The reported #471 shape: the surface feeds an effect dependency that the
    // render reads, so an absent surface throws before any markup exists.
    function ChatSurface() {
      const surface = useSessionBindingSurface("tab-a", null);
      const [recoveryOpen, setRecoveryOpen] = useState(true);
      useEffect(() => {
        if (!surface.showRecoveryAction) setRecoveryOpen(false);
      }, [surface.showRecoveryAction]);
      return (
        <div data-testid="chat-surface">
          {recoveryOpen ? "recovery" : "empty"}
        </div>
      );
    }

    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(
      <RenderGuard>
        <ChatSurface />
      </RenderGuard>,
    ));
    await act(async () => {
      await subscribed;
    });
    assert(deliver, "the surface subscribed to the daemon binding event");
    await act(async () => (deliver as (fact: SessionBindingFact) => void)(unknownKindFact()));

    assert(!host.querySelector('[data-testid="render-guard-tripped"]'), "an unknown daemon kind must not throw the render");
    const surface = host.querySelector('[data-testid="chat-surface"]');
    assert(surface, "the chat surface still renders after the unknown kind arrives");
    assert(surface.textContent === "empty", "the unknown kind settles on the empty state, not a recovery banner");

    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }],
  ["binding_state_surface subscription is event-driven with no timer", () => {
    const source = readFileSync(
      fileURLToPath(new URL("./useSessionBindingSurface.ts", import.meta.url)),
      "utf8",
    );
    for (const timer of ["setInterval", "setTimeout", "requestAnimationFrame", "Date.now"]) {
      assert(!source.includes(timer), `the surface subscription must not reach for ${timer}`);
    }
    assert(source.includes("session-binding-state-changed"), "correction arrives on the daemon event");
  }],
  ["binding_state_surface reaches rebind through the visible repair mechanism", async () => {
    installDom();
    const { __setInvokeForTest } = await import("../../lib/bridge");
    const { AgentStateIndicator } = await import("./AgentStateIndicator");
    const { SessionBindingRecoveryForm } = await import("./SessionBindingRecoveryForm");
    const commands: Array<{ command: string; args?: Record<string, unknown> }> = [];
    let recorded: () => void = () => undefined;
    const firstCommand = new Promise<void>((resolve) => {
      recorded = resolve;
    });
    const invokeMock = async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
      commands.push({ command, args });
      recorded();
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
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      await firstCommand;
    });
    assert(commands.some(({ command, args }) => command === "rebind_tab_session"
      && args?.tabId === "tab-a"
      && args?.sessionId === "requested-session"), "the form invokes the tab rebind mechanism");
    await act(async () => root.unmount());
    __setInvokeForTest(null);
  }],
];

async function main(): Promise<void> {
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`PASS ${name}`);
    } catch (error) {
      console.error(`FAIL ${name}`);
      throw error;
    }
  }
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
