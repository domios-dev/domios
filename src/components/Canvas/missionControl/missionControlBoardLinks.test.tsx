import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { DEEP_LINK_UNAVAILABLE_COPY } from "../../../lib/notificationDeepLink";
import type { ApiConfig, Tab, Workspace } from "../../../lib/types";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import { MissionControlPlanSurface } from "./MissionControlPlanSurface";
import { GATE_NO_TAB_COPY } from "./missionControlBoardModel";
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

function gate(tabId: string | null): OpenDecisionGate {
  return {
    id: "gt_1",
    question: `Resolve the provider incident for ${ACTIVE.id}`,
    status: "open",
    asked_at: 0,
    age_ms: 3_600_000,
    blocking_task_count: 1,
    tab_id: tabId,
  };
}

function deepLinks(onSelectTab: NeedsYouDeepLinks["onSelectTab"]): NeedsYouDeepLinks {
  return {
    notifications: [],
    tabs: { "tab-hopper": { id: "tab-hopper", workspace_id: "ws-1", title: "Hopper" } as unknown as Tab },
    workspaces: [],
    scope: { teamId: null, workspaceId: null },
    onSelectTab,
  };
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
  it("mission_control_previous_plans_open_the_archived_plan_detail_by_click_and_keyboard", async () => {
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
    try {
      const { unmount } = await mount(
        <MissionControlPlanSurface mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[ACTIVE]} api={API} />,
      );
      await settle();

      const toggle = document.querySelector('[data-testid="mc-board-previous-plans-toggle"]') as HTMLButtonElement;
      expect(toggle).not.toBeNull();
      const entry = () =>
        document.querySelector(`[data-testid="mc-board-previous-plan"][data-plan-id="${ARCHIVED.id}"]`) as HTMLButtonElement;
      const detailTitle = () => document.querySelector('[data-testid="plan-detail-title"]')?.textContent;

      await act(async () => toggle.click());
      await act(async () => entry().click());
      expect(detailTitle()).toBe(ARCHIVED.title);

      await act(async () => keydown(document.body, "Escape"));
      expect(document.querySelector('[data-testid="plan-detail-title"]')).toBeNull();

      await act(async () => toggle.click());
      expect(document.activeElement).toBe(entry());
      await act(async () => keydown(entry(), "Enter"));
      expect(detailTitle()).toBe(ARCHIVED.title);
      expect(document.querySelector('[data-testid="mc-board-previous-plans-menu"]')).toBeNull();
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

describe("mission control decision gate rows", () => {
  it("mission_control_gate_row_jumps_to_its_tab_by_click_and_enter_while_names_keep_their_links", async () => {
    const onSelectTab = vi.fn();
    const onOpenPlan = vi.fn();
    const { host, unmount } = await mount(
      <MissionControlBoard
        plan={ACTIVE}
        plans={[ACTIVE]}
        state={boardState([gate("tab-hopper")])}
        deepLinks={deepLinks(onSelectTab)}
        onOpenPlan={onOpenPlan}
      />,
    );

    const link = host.querySelector('[data-testid="mc-board-gate-link"]') as HTMLElement;
    expect(link.getAttribute("role")).toBe("link");
    expect(link.tabIndex).toBe(0);
    expect(link.querySelectorAll("button").length).toBe(1);

    await act(async () => link.click());
    expect(onSelectTab).toHaveBeenLastCalledWith("ws-1", "tab-hopper");
    await act(async () => keydown(link, "Enter"));
    expect(onSelectTab).toHaveBeenCalledTimes(2);

    const planName = link.querySelector(`[data-ref-id="${ACTIVE.id}"]`) as HTMLButtonElement;
    expect(planName.textContent).toBe(ACTIVE.title);
    await act(async () => planName.click());
    await act(async () => keydown(planName, "Enter"));
    expect(onOpenPlan).toHaveBeenCalledWith(ACTIVE.id);
    expect(onSelectTab).toHaveBeenCalledTimes(2);
    await unmount();
  });

  it("mission_control_gate_row_without_a_live_tab_is_not_a_link_and_says_why", async () => {
    for (const [tabId, reason] of [
      [null, GATE_NO_TAB_COPY],
      ["tab-gone", DEEP_LINK_UNAVAILABLE_COPY.tab_gone],
    ] as const) {
      const onSelectTab = vi.fn();
      const { host, unmount } = await mount(
        <MissionControlBoard plan={ACTIVE} plans={[ACTIVE]} state={boardState([gate(tabId)])} deepLinks={deepLinks(onSelectTab)} />,
      );

      const row = host.querySelector('[data-kind="gate"]') as HTMLElement;
      expect(row.getAttribute("data-gate-link")).toBe("unavailable");
      expect(row.querySelector('[role="link"]')).toBeNull();
      expect(row.querySelector('[data-testid="mc-board-gate-unavailable"]')?.textContent).toBe(reason);
      await act(async () => row.click());
      await act(async () => keydown(row, "Enter"));
      expect(onSelectTab).not.toHaveBeenCalled();
      await unmount();
    }
  });
});
