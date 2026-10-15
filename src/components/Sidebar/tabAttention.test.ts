import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Notification, Tab } from "../../lib/types";
import { attentionActions, tabAttention, tabAttentionByTab, tabStatusMark, tabStartingLabel } from "./tabAttention";

const tab: Tab = { id: "agent", title: "Agent", pty_id: null, tab_type: "Terminal", cwd: null };
function notice(id: string, level: Notification["level"], overrides: Partial<Notification> = {}): Notification {
  return { id, tab_id: tab.id, workspace_id: "workspace", title: `${id}-title`, body: `${id}-body`, severity: "Info", level, read: false, timestamp: 1, ...overrides };
}

describe("tab attention model", () => {
  it("projects each source and its reason", () => {
    const waiting = { ...tab, pending_prompt: { question: "prompt-token", options: [] } };
    expect(tabAttention(waiting, null, []).reason).toBe(waiting.pending_prompt.question);
    expect(tabAttention(waiting, null, []).kind).toBe("waiting");
    expect(tabAttention(tab, "Waiting", []).kind).toBe("waiting");
    for (const level of ["action_needed", "failure"] as const) {
      const notification = notice(level, level);
      const model = tabAttention(tab, null, [notification]);
      expect(model.kind).toBe(level);
      expect(model.reason).toBe(notification.title);
      expect(model.body).toBe(notification.body);
      expect(model.notification).toBe(notification);
    }
  });
  it("prioritizes failure over action over waiting, then newest within a kind", () => {
    const notifications = [notice("failure", "failure"), notice("action", "action_needed", { timestamp: 10 })];
    expect(tabAttention(tab, "Waiting", notifications).kind).toBe("failure");
    expect(tabAttention(tab, "Waiting", notifications.slice(1)).kind).toBe("action_needed");
    expect(tabAttention(tab, "Waiting", []).kind).toBe("waiting");
    expect(tabAttention(tab, null, [notifications[0], notice("new-failure", "failure", { timestamp: 2 })]).notification?.id).toBe("new-failure");
  });
  it("clears read notices and ignores other tabs, verbose notices and resolved notices", () => {
    const notification = notice("n", "action_needed");
    expect(tabAttention(tab, null, [notification]).showMark).toBe(true);
    const cleared = tabAttention(tab, null, [{ ...notification, read: true }]);
    expect(cleared.cleared).toBe(true);
    expect(cleared.showMark).toBe(false);
    expect(cleared.wantsAttention).toBe(false);
    expect(tabAttention(tab, null, [notice("v", "verbose"), notice("other", "failure", { tab_id: "other" }), notice("resolved", "failure", { resolved: true })]).kind).toBe(null);
  });
  it("suppresses active marks but retains the reason and restores live waiting on leaving", () => {
    const notification = notice("n", "failure");
    const active = tabAttention(tab, "Waiting", [notification], true);
    expect(active.showMark).toBe(false);
    expect(active.reason).toBe(notification.title);
    expect(active.notificationIds).toEqual([notification.id]);
    expect(tabAttention(tab, "Waiting", [], true).cleared).toBe(true);
    expect(tabAttention(tab, "Waiting", [], false).showMark).toBe(true);
    expect(tabAttentionByTab({ [tab.id]: tab }, { [tab.id]: "Waiting" }, [notification])[tab.id].kind).toBe("failure");
  });
  it("selects one icon mark with failure and attention ahead of startup", () => {
    const starting = { ...tab, agent_type: "fixture", boot_stage: "agent_launching" as const };
    const model = tabAttention(starting, "Working", []);
    expect(tabStatusMark(tab.title, model)?.kind).toBe("starting");
    expect(tabStatusMark(tab.title, model, true)?.kind).toBe("starting");
    const connecting = tabAttention({ ...starting, boot_stage: "launch_binding_unconfirmed" }, "Working", []);
    expect(connecting.starting).toBe("connecting");
    expect(tabStartingLabel(starting)).toBe(tabStatusMark(starting.title, model)?.label);
    expect(tabStartingLabel({ ...starting, boot_stage: "launch_binding_unconfirmed" })).toBe(tabStatusMark(starting.title, connecting)?.label);
    for (const level of ["action_needed", "failure"] as const) {
      const mark = tabStatusMark(tab.title, tabAttention(starting, "Working", [notice(level, level)]));
      expect(mark?.kind).toBe("attention");
      expect(mark?.kind === "attention" ? mark.level : null).toBe(level);
    }
  });
  it("uses the icon clock for frozen tabs and clears it on wake", () => {
    const sleeping = tabAttention({ ...tab, frozen: true }, null, []);
    expect(tabStatusMark(tab.title, sleeping)).toEqual({ kind: "sleeping", label: "Sleeping" });
    expect(tabStatusMark(tab.title, sleeping, true)).toEqual({ kind: "sleeping", label: "Sleeping" });
    expect(tabStatusMark(tab.title, tabAttention({ ...tab, frozen: false }, null, []))).toBe(null);
    const starting = { ...tab, frozen: false, agent_type: "fixture", boot_stage: "agent_launching" as const };
    expect(tabStatusMark(tab.title, tabAttention(starting, null, []))?.kind).toBe("starting");
  });
  it("does not offer stale choices after resolution, answering or loss of the asking tab", () => {
    const n = notice("choice", "action_needed", { actions: [{ type: "answer", label: "choice-token" }] });
    expect(attentionActions(n)).toEqual(n.actions);
    expect(attentionActions({ ...n, resolved: true })).toEqual([]);
    expect(attentionActions({ ...n, answer: { answered: "choice-token", delivered: true, asked_from_tab: tab.id } })).toEqual([]);
    expect(attentionActions({ ...n, answer: { answered: null, delivered: false, asked_from_tab: null } })).toEqual([]);
  });
});
finish();
