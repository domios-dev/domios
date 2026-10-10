import { act } from "react";
import { MissionControlTeamBoard } from "../Canvas/missionControl/MissionControlTeamBoard";
import { MissionControlBoard } from "../Canvas/missionControl/MissionControlBoard";
import { FIXTURE_PLAN } from "../Canvas/cards/fixtures.testdata";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ActivityFeed } from "./ActivityFeed";
import type { SupporterActivity } from "./activity";
import { API, harness } from "./testSupport";

vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  daemonFetch: (_api: unknown, path: string, init: RequestInit) => fetch(path, init),
  daemonAuthToken: () => "fixture",
}));
vi.mock("../Canvas/missionControl/BoardCards", () => ({
  SpendCard: () => <div data-testid="team-spend" />,
  ModelsCard: () => null, PlanCard: () => null, NeedsYouPanel: () => null, NoActivePlanCard: () => null,
}));
vi.mock("../Canvas/missionControl/BoardEventLog", () => ({ BoardEventLog: () => null }));
let setup: ReturnType<typeof harness>;
let entries: SupporterActivity[];
let paths: string[];
function entry(team: string, time = 1700000000): SupporterActivity {
  return { time, team_id: team, kind: "tab_state", label: "other", confidence_band: "strong", action: "notify_only", reason: "launch_ready", budget_reached: false, fallback: false };
}
beforeEach(() => {
  setup = harness();
  entries = [entry("team_aabbccdd"), entry("team_11223344", 1700000001)];
  paths = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    paths.push(String(input));
    return new Response(JSON.stringify({ entries }), { status: 200 });
  });
});
afterEach(async () => setup.cleanup());

describe("Spinal Cord activity", () => {
  it("heartbeat_wakes_and_suppressions_render_in_the_feed", async () => {
    entries = [
      { ...entry("team_aabbccdd"), kind: "heartbeat", label: "action_needed", action: "wake", reason: "hard_wake" },
      { ...entry("team_aabbccdd"), kind: "heartbeat", label: "nothing_changed", action: "suppress", reason: "same_snapshot" },
    ];
    const host = await setup.render(<ActivityFeed api={API} />);
    const rows = host.querySelectorAll("ol > li");
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute("data-kind")).toBe("heartbeat");
    expect(rows[0].textContent).toContain("Action: wake");
    expect(rows[1].textContent).toContain("Action: suppress");
  });
  it("feed_filters_by_team", async () => {
    const host = await setup.render(<ActivityFeed api={API} teamId="team_aabbccdd" />);
    expect(new URL(paths[0], "http://fixture").searchParams.get("team_id")).toBe("team_aabbccdd");
    expect(new URL(paths[0], "http://fixture").searchParams.get("limit")).toBe("50");
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    expect(host.querySelector("time")?.dateTime).toBe("2023-11-14T22:13:20.000Z");
    expect(host.querySelector("li")?.getAttribute("data-confidence")).toBe("strong");
  });
  it("machine_feed_lists_all_decisions_and_recovery", async () => {
    entries[1].kind = "recovery";
    entries[1].action = "recovery";
    entries[1].fallback = true;
    const host = await setup.render(<ActivityFeed api={API} />);
    expect(new URL(paths[0], "http://fixture").searchParams.has("team_id")).toBe(false);
    expect(host.querySelectorAll("ol > li")).toHaveLength(2);
    expect(host.querySelectorAll("li")[1].getAttribute("data-kind")).toBe("recovery");
    expect(host.querySelectorAll("li")[1].getAttribute("data-fallback")).toBe("true");
  });
  it("sentinel_verdicts_and_scheduler_decisions_are_visible_with_safe_fields", async () => {
    entries = [
      { ...entry("team_aabbccdd"), kind: "sentinel", label: "private-bucket", action: "decision", reason: "transition" },
      { ...entry("team_aabbccdd"), kind: "sentinel", label: "private-bucket", action: "notify_only", reason: "model_answer" },
      { ...entry("team_aabbccdd"), kind: "sentinel", label: "private-bucket", action: "decision", reason: "budget_exhausted", budget_reached: true, fallback: true },
      { ...entry("team_11223344"), kind: "sentinel", action: "send", reason: "route_failed" },
    ];
    const host = await setup.render(<ActivityFeed api={API} teamId="team_aabbccdd" />);
    const rows = host.querySelectorAll("ol > li");
    expect(rows).toHaveLength(3);
    for (const row of rows) {
      expect(row.getAttribute("data-kind")).toBe("sentinel");
      expect(row.getAttribute("data-confidence")).toBe("strong");
      expect(row.textContent).toContain("sentinel · redacted · strong confidence");
    }
    expect(rows[0].textContent).toContain("Action: decision");
    expect(rows[0].textContent).toContain("Reason: transition");
    expect(rows[1].textContent).toContain("Action: notify only");
    expect(rows[1].textContent).toContain("Reason: model answer");
    expect(rows[2].textContent).toContain("Reason: budget exhausted");
    expect(rows[2].textContent).toContain("Budget: reached");
    expect(rows[2].getAttribute("data-fallback")).toBe("true");
    expect(host.innerHTML).not.toContain("private-bucket");
    expect(host.textContent).not.toContain("route failed");
  });
  it("feed_redacts_screen_and_token_content", async () => {
    const screen = "private shell output\nrequest body";
    const token = "sk-private-opaque-token";
    entries = [{ ...entry("team_aabbccdd"), label: screen, reason: token, action: token, confidence_band: screen, kind: token }];
    vi.mocked(fetch).mockImplementation(async () => new Response(JSON.stringify({ entries: [{ ...entries[0], screen, token, body: screen, distribution: { choice: token } }] }), { status: 200 }));
    const host = await setup.render(<ActivityFeed api={API} />);
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    expect(host.innerHTML).not.toContain(screen);
    expect(host.innerHTML).not.toContain(token);
    expect(host.querySelector("li")?.getAttribute("data-confidence")).toBe("redacted");
    expect(host.querySelector("li")?.getAttribute("data-kind")).toBe("redacted");
  });
  it("feed_refreshes_on_journal_event", async () => {
    const host = await setup.render(<ActivityFeed api={API} teamId="team_aabbccdd" />);
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    entries.push(entry("team_aabbccdd", 1700000002));
    await setup.emit();
    expect(host.querySelectorAll("ol > li")).toHaveLength(2);
    expect(paths).toHaveLength(2);
    await setup.cleanup();
    expect(setup.listeners.size).toBe(0);
  });
  it("feed_recovers_after_fetch_failure_on_journal_event", async () => {
    vi.mocked(fetch).mockRejectedValueOnce(new Error("offline"));
    const host = await setup.render(<ActivityFeed api={API} />);
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(host.querySelectorAll("ol > li")).toHaveLength(0);
    await setup.emit();
    expect(host.querySelectorAll("ol > li")).toHaveLength(2);
    expect(host.querySelector('[role="status"]')).toBeNull();
  });
  it("feed_team_switch_rejects_a_late_previous_team_response", async () => {
    let resolve!: (response: Response) => void;
    const pending = new Promise<Response>((next) => { resolve = next; });
    vi.mocked(fetch).mockReturnValueOnce(pending);
    const host = await setup.render(<ActivityFeed api={API} teamId="team_aabbccdd" />);
    await setup.rerender(<ActivityFeed api={API} teamId="team_11223344" />);
    expect(host.querySelector("time")?.dateTime).toBe("2023-11-14T22:13:21.000Z");
    await act(async () => { resolve(new Response(JSON.stringify({ entries: [entry("team_aabbccdd")] }), { status: 200 })); });
    expect(host.querySelectorAll("li")).toHaveLength(1);
    expect(host.querySelector("time")?.dateTime).toBe("2023-11-14T22:13:21.000Z");
  });

  it("team_board_places_the_filtered_feed_next_to_team_spend", async () => {
    const host = await setup.render(<MissionControlTeamBoard api={API} teamId="team_aabbccdd" team={{ activePlan: null, archivedPlans: [], events: { pageCount: 1, page: () => [], caption: "" } }} />);
    expect(host.querySelector('[data-testid="team-spend"]')).not.toBeNull();
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    expect(new URL(paths[0], "http://fixture").searchParams.get("team_id")).toBe("team_aabbccdd");
  });
  it("active_plan_board_feed_does_not_depend_on_plan_projection_readiness", async () => {
    const host = await setup.render(<MissionControlBoard api={API} plan={{ ...FIXTURE_PLAN, team_id: "team_aabbccdd" }} state={null} />);
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    expect(new URL(paths[0], "http://fixture").searchParams.get("team_id")).toBe("team_aabbccdd");
  });

  it("rows_show_a_one_line_summary_and_expand_to_details", async () => {
    entries = [{ ...entry("team_aabbccdd"), kind: "rerank", label: "ranked", action: "rerank", reason: "all_candidates_scored" }];
    const host = await setup.render(<ActivityFeed api={API} />);
    const row = host.querySelector("ol > li")!;
    expect(row.querySelector("summary")?.textContent).toContain("Memory re-rank: ranked");
    const details = row.querySelector("details")!;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain("Reason: all candidates scored");
  });

  it("summary_shows_question_to_answer", async () => {
    const question = "supplied question";
    const answer = "supplied answer";
    entries = [{ ...entry("team_aabbccdd"), asked: [{ name: "fixture", question, answer }] }];
    const host = await setup.render(<ActivityFeed api={API} />);
    const row = host.querySelector("ol > li")!;
    expect(row.querySelector("summary")?.textContent).toContain(`${question} → ${answer}`);
    expect(row.querySelector("details")?.open).toBe(false);
    expect(row.querySelector("summary")?.textContent).not.toContain("launch ready");
  });

  it("row_expands_to_each_question", async () => {
    const asked = [
      { name: "label", question: "fixture state question", answer: "fixture state answer" },
      { name: "prompt_visible", question: "fixture visibility question", answer: "fixture visibility answer" },
      { name: "safe_choice", question: "fixture safety question", answer: "fixture safety answer" },
    ];
    entries = [{ ...entry("team_aabbccdd"), asked }];
    const host = await setup.render(<ActivityFeed api={API} />);
    expect(host.querySelectorAll("ol > li")).toHaveLength(1);
    const details = host.querySelector("details")!;
    expect(details.open).toBe(false);
    details.open = true;
    details.dispatchEvent(new Event("toggle"));
    expect(details.open).toBe(true);
    const pairs = details.querySelector('dl[aria-label="Questions and answers"]')!;
    expect(Array.from(pairs.querySelectorAll("dt"), (node) => node.textContent)).toEqual(asked.map((pair) => pair.question));
    expect(Array.from(pairs.querySelectorAll("dd"), (node) => node.textContent)).toEqual(asked.map((pair) => pair.answer));
    expect(host.querySelector("summary")?.textContent).toContain(asked[0].answer);
    expect(host.querySelector("summary")?.textContent).not.toContain(asked[1].question);
  });

  it("details_keep_confidence_budget_fallback", async () => {
    entries = [{ ...entry("team_aabbccdd"), confidence_band: "weak", budget_reached: true, fallback: true,
      reason: "budget", asked: [{ name: "fixture", question: "supplied question", answer: "supplied answer" }],
    }];
    const host = await setup.render(<ActivityFeed api={API} />);
    const details = host.querySelector("details")!;
    expect(details.textContent).toContain("weak confidence");
    expect(details.textContent).toContain("Budget: reached");
    expect(details.textContent).toContain("deterministic fallback");
    expect(details.textContent).toContain("Action: notify only");
    expect(details.textContent).toContain("Reason: budget");
  });

});
