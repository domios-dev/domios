import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { ApiConfig, Workspace } from "../../../lib/types";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import { MissionControlPlanSurface } from "./MissionControlPlanSurface";
import type { MissionControlState, OpenDecisionGate, PlanBoardState, PlanTaskCounts } from "./missionControlState";
import { planForDetail } from "./planDetailTarget";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false };

const WS_ID = "ws-test";
const WORKSPACES: Workspace[] = [{ id: WS_ID, name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null }];

const ACTIVE: Plan = { ...FIXTURE_PLAN, id: "pl_snake", title: "Animated Snake game", status: "active", team_id: "team-1" };

const ARCHIVED: Plan = {
  ...FIXTURE_PLAN,
  id: "pl_verify",
  title: "CodeTerm 1.10.17 verification Sep 12",
  status: "archived",
  team_id: "team-1",
  tasks: [],
};

const COUNTS: PlanTaskCounts = {
  pending: 1,
  in_progress: 0,
  in_review: 0,
  blocked: 0,
  paused: 0,
  done: 0,
  cancelled: 0,
  total: 1,
};

function missionState(openGates: OpenDecisionGate[] = []): MissionControlState {
  return {
    held: { held_reviews: [], open_gates: openGates, oldest_hold_age_ms: null, verdicts_on_held: 0 },
    bounces: { total_attempts: 0, tasks_on_second_attempt: 0, first_try_done: 0, labelled_bounced: 0 },
    event_log: { recorded_events: 0, truncated: false, entries: [] },
    areas: { state: "no_facet", facet_key: null, facet_label: null, groups: [], untagged_task_count: 0, role_fallback: [] },
    kpi: { state: "not_recording", not_recording_reason: "disabled", enable_setting: null, spend: null },
    spend: {
      scope: "machine",
      state: "not_recording",
      not_recording_reason: "disabled",
      enable_setting: null,
      measured_tokens: 0,
      turns: 0,
      providers: [],
    },
  };
}

function boardState(openGates: OpenDecisionGate[] = []): PlanBoardState {
  return { plan_id: ACTIVE.id, task_counts: COUNTS, mission_control: missionState(openGates) };
}

async function mount(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

/** Lets every stubbed read resolve and its state update commit; reads are promise chains, never timers. */
async function settle() {
  for (let pass = 0; pass < 10; pass += 1) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

function keydown(target: Element, key: string) {
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}

function respond(body: unknown) {
  return { ok: true, status: 200, json: async () => body, text: async () => "" };
}

describe("mission control previous plans", () => {
  it("mission_control_previous_plans_open_the_archived_plan_through_the_navigation_owner_by_click_and_keyboard", async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes(`/api/plans/${ACTIVE.id}/state`)) return respond(boardState());
      if (url.includes("/mission-control/state")) {
        return respond({
          team_id: "team-1",
          team: {},
          active_plan: ACTIVE,
          archived_plans: [ARCHIVED],
          events: [],
          next_cursor: null,
        });
      }
      return { ok: false, status: 404, json: async () => ({}), text: async () => "not found" };
    });
    vi.stubGlobal("fetch", fetchMock);
    const onOpenPlanTarget = vi.fn();
    try {
      const { unmount } = await mount(
        <MissionControlPlanSurface
          mode="workspace"
          workspaceId={WS_ID}
          workspaces={WORKSPACES}
          plans={[ACTIVE]}
          api={API}
          onOpenPlanTarget={onOpenPlanTarget}
        />,
      );
      await settle();

      const toggle = document.querySelector('[data-testid="mc-board-previous-plans-toggle"]') as HTMLButtonElement;
      expect(toggle).not.toBeNull();
      const entry = () =>
        document.querySelector(`[data-testid="mc-board-previous-plan"][data-plan-id="${ARCHIVED.id}"]`) as HTMLButtonElement;
      const archivedTarget = { planId: ARCHIVED.id, teamId: "team-1" };

      await act(async () => toggle.click());
      await act(async () => entry().click());
      expect(onOpenPlanTarget).toHaveBeenLastCalledWith(archivedTarget);

      await act(async () => toggle.click());
      expect(document.activeElement).toBe(entry());
      await act(async () => keydown(entry(), "Enter"));
      expect(onOpenPlanTarget).toHaveBeenCalledTimes(2);
      expect(onOpenPlanTarget).toHaveBeenLastCalledWith(archivedTarget);
      expect(document.querySelector('[data-testid="mc-board-previous-plans-menu"]')).toBeNull();
      expect(document.querySelector('[data-testid="plan-detail-overlay"]')).toBeNull();
      await unmount();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("mission_control_plan_detail_opens_nothing_for_an_unknown_plan_id", () => {
    expect(planForDetail("pl_missing", ACTIVE, [ACTIVE], [ARCHIVED])).toBeNull();
    expect(planForDetail(ARCHIVED.id, ACTIVE, [ACTIVE], [ARCHIVED])).toBe(ARCHIVED);
    expect(planForDetail(undefined, ACTIVE, [ACTIVE], [ARCHIVED])).toBe(ACTIVE);
  });

  it("mission_control_previous_plans_menu_floats_in_a_layer_outside_the_card", async () => {
    const { host, unmount } = await mount(
      <MissionControlBoard plan={ACTIVE} previousPlans={[ARCHIVED]} state={boardState()} />,
    );

    await act(async () => (host.querySelector('[data-testid="mc-board-previous-plans-toggle"]') as HTMLButtonElement).click());
    const menu = document.querySelector('[data-testid="mc-board-previous-plans-menu"]') as HTMLElement;
    expect(menu.parentElement).toBe(document.body);
    expect(host.contains(menu)).toBe(false);
    expect(menu.classList.contains("mc-board-layer")).toBe(true);
    expect(menu.style.left).not.toBe("");

    await act(async () => document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
    expect(document.querySelector('[data-testid="mc-board-previous-plans-menu"]')).toBeNull();
    await unmount();
  });
});
