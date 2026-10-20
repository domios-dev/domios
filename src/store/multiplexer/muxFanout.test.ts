import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { buildInitialState } from "./initialState";
import { reducer } from "./reducer";
import { snapWithTabs } from "./reducer.test.fixtures";
import { remoteControllerSignature } from "./remoteTabProjection";
import { appShellStateEqual, shellTabsEqual } from "./selectors";

const tab = (id: string, extra: Partial<Tab> = {}): Tab => ({ id, title: id, pty_id: null, tab_type: "Terminal", cwd: null, ...extra });

describe("remote controller signature", () => {
  const lead = tab("lead", { workspace_id: "w1", team_id: "t1", agent_state: { state: "working", status_line: "1s" } });
  const viewer = tab("mesh:peer/worker", { remote_peer_id: "peer", parent_tab_id: "lead" });

  it("ignores live telemetry on controllers and viewers", () => {
    const before = remoteControllerSignature({ lead, [viewer.id]: viewer });
    const after = remoteControllerSignature({
      lead: { ...lead, last_active_at_ms: 9, agent_state: { state: "working", status_line: "2s" } },
      [viewer.id]: { ...viewer, agent_state: { state: "idle" } },
    });
    expect(after).toBe(before);
  });

  it("changes with what the projection copies from a controller", () => {
    const before = remoteControllerSignature({ lead, [viewer.id]: viewer });
    expect(remoteControllerSignature({ lead: { ...lead, title: "Lead" }, [viewer.id]: viewer })).not.toBe(before);
    expect(remoteControllerSignature({ lead: { ...lead, workspace_id: "w2" }, [viewer.id]: viewer })).not.toBe(before);
    expect(remoteControllerSignature({ lead: { ...lead, team_id: null }, [viewer.id]: viewer })).not.toBe(before);
    expect(remoteControllerSignature({ [viewer.id]: viewer })).not.toBe(before);
  });

  it("is constant for local tabs without remote viewers", () => {
    expect(remoteControllerSignature({ lead })).toBe(remoteControllerSignature({ lead: { ...lead, title: "Renamed" } }));
  });
});

describe("app shell tab structure", () => {
  const first = reducer(buildInitialState(), { type: "SET_STATE", state: snapWithTabs(1, [["w1", "a"], ["w1", "b"]]) });
  const withTab = (id: string, change: Partial<Tab>) => ({ ...first, tabs: { ...first.tabs, [id]: { ...first.tabs[id], ...change } } });

  it("stays equal when only live telemetry changes", () => {
    const streamed = withTab("b", { last_active_at_ms: 42, agent_state: { state: "working", status_line: "Thinking 3s" } });
    expect(shellTabsEqual(first.tabs, streamed.tabs)).toBe(true);
    expect(appShellStateEqual(first, streamed)).toBe(true);
  });

  it("stays equal when a streamed snapshot rebuilds unchanged nested fields", () => {
    const nested = () => ({ provider_signals: { api_error: null }, turn_resume: { pending: false } }) as unknown as Partial<Tab>;
    const base = withTab("b", nested());
    const rebuilt = { ...base, tabs: { ...base.tabs, b: { ...base.tabs.b, ...nested(), agent_state: { state: "working" as const, status_line: "4s" } } } };
    expect(shellTabsEqual(base.tabs, rebuilt.tabs)).toBe(true);
  });

  it("changes when a tab's structure changes or a tab leaves", () => {
    expect(appShellStateEqual(first, withTab("b", { title: "Renamed" }))).toBe(false);
    const { b: _removed, ...rest } = first.tabs;
    expect(shellTabsEqual(first.tabs, rest)).toBe(false);
  });
});
