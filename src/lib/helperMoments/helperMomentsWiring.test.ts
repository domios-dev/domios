import { afterEach, describe, expect, it } from "vitest";
import { __setListenForTest } from "../bridge";
import type { Tab } from "../types";
import { createHelperMomentsStore } from "./helperMomentsStore";
import { helperTabKind, pluginFacts, startHelperMoments } from "./helperMomentsWiring";

type Handler = (event: { payload: unknown }) => void;

function fakeEvents() {
  const handlers = new Map<string, Handler>();
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as Handler);
    return () => handlers.delete(event);
  });
  return {
    handlers,
    emit: (event: string, payload: unknown) => handlers.get(event)?.({ payload }),
  };
}

const flush = () => new Promise<void>((resolve) => queueMicrotask(resolve));

afterEach(() => __setListenForTest(null));

describe("helper moments wiring", () => {
  it("classifies agent tabs, a person's own shell, and agent-driven shells", () => {
    expect(helperTabKind({ agent_type: "claude" } as Tab)).toBe("agent");
    expect(helperTabKind({ agent_type: null } as Tab)).toBe("manual-shell");
    expect(helperTabKind({ agent_type: null, team_id: "team" } as Tab)).toBe("other");
    expect(helperTabKind({ agent_type: null, purpose: "build" } as Tab)).toBe("other");
    expect(helperTabKind(undefined)).toBe("other");
  });

  it("reports plugin facts only once the catalog has loaded", () => {
    expect(pluginFacts({ hasLoaded: false, snapshot: { plugins: [], channels: [], diff: [], marketplace: [] } })).toBeNull();
    const facts = pluginFacts({
      hasLoaded: true,
      snapshot: { plugins: [], channels: [], diff: [], marketplace: [{ id: "jira-client", channel: "c", display_name: "Jira", installed: false, compatible: true } as never] },
    });
    expect(facts).toEqual([{ id: "jira-client", name: "Jira", installed: false }]);
  });

  it("turns existing focus, tab-status and shell-command events into moments", async () => {
    const events = fakeEvents();
    const store = createHelperMomentsStore({
      load: () => null,
      save: () => {},
      tabKind: (tabId) => (tabId === "agent" ? "agent" : "manual-shell"),
      blueprintsInUse: async () => false,
    });
    const stop = startHelperMoments(store);
    await flush();
    store.setFacts({ telegramConnected: false });
    expect([...events.handlers.keys()].sort()).toEqual(["shell-command-changed", "tab-status-changed"]);

    window.dispatchEvent(new Event("blur"));
    events.emit("tab-status-changed", { tab_id: "agent", status: "Working" });
    events.emit("shell-command-changed", { tab_id: "shell", event: { kind: "started", commandLine: "make" } });
    expect(store.getSnapshot().active).toBeNull();

    for (const commandLine of ["make", "./deploy.sh", "make", "./deploy.sh", "make", "./deploy.sh"]) {
      events.emit("shell-command-changed", { tab_id: "shell", event: { kind: "finished", commandLine, exitCode: 0 } });
    }
    await flush();
    await flush();
    expect(store.getSnapshot().active?.momentId).toBe("repeated_manual_action");

    stop();
    expect(events.handlers.size).toBe(0);
  });
});
