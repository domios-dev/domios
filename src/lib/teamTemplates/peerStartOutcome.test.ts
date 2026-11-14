import { describe, expect, it } from "vitest";
import exchange from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/exchange.json";
import type { Tab } from "../types";
import { emptyHostReason, hostListingFailure, NO_HOST_PEER, peerWorkspacesOf } from "./hostPeers";
import { managersToAttach, peerOutcomeError, type TemplatePeerStartOutcome } from "./peerStartOutcome";

const started = exchange.peer_start_finished.outcome as TemplatePeerStartOutcome;
const peerTab = (id: string): Tab => ({ id, title: id, pty_id: `pty-${id}`, tab_type: "Terminal", cwd: null });

describe("peer Team start", () => {
  it("attaches only a started Team's managers, Planner first, and only those the peer lists", () => {
    const tabs = [peerTab("t_orch"), peerTab("t_plan"), peerTab("other")];
    expect(managersToAttach(started, tabs).map((tab) => tab.id)).toEqual(["t_plan", "t_orch"]);
    expect(managersToAttach(started, [peerTab("t_orch")]).map((tab) => tab.id)).toEqual(["t_orch"]);
    expect(managersToAttach({ ...started, state: "failed" }, tabs)).toEqual([]);
  });

  it("says plainly why a peer start did not start a Team", () => {
    expect(peerOutcomeError({ ...started, state: "failed", error: "ERROR-FRAGMENT" }).message).toContain("ERROR-FRAGMENT");
    expect(peerOutcomeError({ ...started, state: "declined" }).message).toContain("eight");
    expect(peerOutcomeError({ ...started, state: "stream_lost" }).message).toContain("no answer");
  });

  it("names the account-mesh refusal, or a listing failure, instead of an empty machine list", () => {
    expect(emptyHostReason(null)).toBe(NO_HOST_PEER);
    expect(emptyHostReason({ reason: "proof_invalid" })).toMatch(/^This machine can't reach your account mesh: .*proof rejected/);
    expect(hostListingFailure(new Error("FAILURE-FRAGMENT"))).toContain("FAILURE-FRAGMENT");
  });

  it("offers the peer's own workspaces but never its protected home", () => {
    expect(peerWorkspacesOf([
      { id: "home", name: "General", protected: true },
      { id: "ws", name: "Research" },
      { name: "no id" },
    ])).toEqual([{ id: "ws", name: "Research" }]);
    expect(peerWorkspacesOf(null)).toEqual([]);
  });
});
