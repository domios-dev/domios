import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
import { curatorTabAttention, tabAttention, tabStatusMark } from "./tabAttention";
import type { PlanControlLoop } from "../Canvas/planControlLoop";

const tab: Tab = {
  id: "curator", title: "Curator", tab_type: "Terminal", pty_id: "pty", cwd: null,
  role_profile: "curator", agent_type: "fixture", agent_session_id: "own-session",
  session_binding_nonce: "own-nonce", reported_session_binding_nonce: "own-nonce",
  boot_stage: "shell_ready", agent_booting: true,
  agent_state: { state: "working", phase: "work" },
};

describe("Curator icon state", () => {
  it("uses the clock only for a scheduled round, never for a running or suspended round", () => {
    const loop = { mode: "curator", next_at: 2000, running: null, suspended_reason: null, fallback_reason: null } as PlanControlLoop;
    const attention = tabAttention(tab, "Working", []);
    expect(tabStatusMark(tab.title, curatorTabAttention(attention, loop))?.kind).toBe("scheduled");
    expect(tabStatusMark(tab.title, curatorTabAttention(attention, { ...loop, running: { number: 81 } }))).toBeNull();
    const suspended = tabStatusMark(tab.title, curatorTabAttention(attention, { ...loop, suspended_reason: "waiting_on_human" }));
    expect(suspended?.kind).toBe("attention");
    expect(suspended?.label).toContain("waiting on your answer");
  });
  it("shows normal work while the own confirmed session runs despite stale startup", () => {
    expect(tabActivity(tab, "Working").kind).toBe("working");
    expect(tabStatusMark(tab.title, tabAttention(tab, "Working", []))).toBeNull();
  });
  it("keeps an unconfirmed launch in startup rather than pretending it works", () => {
    const unbound = { ...tab, reported_session_binding_nonce: "other" };
    expect(tabActivity(unbound, "Working").kind).toBe("starting");
    expect(tabStatusMark(tab.title, tabAttention(unbound, "Working", []))?.kind).toBe("starting");
  });
  it("does not label a frozen Curator round as a scheduled round", () => {
    const attention = tabAttention({ ...tab, frozen: true, boot_stage: "agent_ready" }, "Idle", []);
    const loop = { mode: "curator", running: { number: 81 } } as PlanControlLoop;
    expect(tabStatusMark(tab.title, curatorTabAttention(attention, loop))).toBeNull();
  });
});
