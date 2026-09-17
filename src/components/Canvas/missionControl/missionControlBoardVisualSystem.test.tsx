import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { MissionControlBoard } from "./MissionControlBoard";
import { BOARD_ROW_COLUMNS, UNASSIGNED_MODEL_LABEL } from "./missionControlBoardModel";
import type { MissionControlState, PlanBoardState } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const NOW = 1_700_000_000_000;
const OWNER_WIDTH = 1456;
const PHONE_WIDTH = 390;
const MAX_CARD_TEXT_STYLES = 4;

const observers: Array<(width: number) => void> = [];

class WidthObserver {
  constructor(private readonly cb: ResizeObserverCallback) {
    observers.push((width) => {
      this.cb(
        [{ contentRect: { width } } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      );
    });
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

(globalThis as { ResizeObserver?: unknown }).ResizeObserver = WidthObserver;

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const BOARD_CSS_PATH = "src/components/Canvas/missionControl/missionControlBoard.css";

function readRepo(file: string): string {
  return readFileSync(path.join(REPO_ROOT, file), "utf8");
}

beforeAll(() => {
  const style = document.createElement("style");
  style.textContent = [readRepo("src/styles/globals.css"), readRepo(BOARD_CSS_PATH)].join("\n");
  document.head.append(style);
});

beforeEach(() => {
  observers.length = 0;
});

/** Selector → declarations for every rule in a stylesheet, comments stripped. */
function cssRules(css: string): Array<{ selector: string; body: string }> {
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return Array.from(stripped.matchAll(/([^{}]+)\{([^{}]*)\}/g)).map((match) => ({
    selector: match[1]!.trim().replace(/\s+/g, " "),
    body: match[2]!,
  }));
}

function rule(css: string, selector: string): string {
  const found = cssRules(css).find((entry) => entry.selector === selector);
  if (!found) throw new Error(`missionControlBoard.css has no rule for ${selector}`);
  return found.body;
}

/** The owner's populated machine: a named Claude model, and Codex with an unresolved and a named model. */
function ownerMission(): MissionControlState {
  return {
    held: { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 },
    bounces: { total_attempts: 2, tasks_on_second_attempt: 1, first_try_done: 0, labelled_bounced: 0 },
    event_log: { recorded_events: 0, truncated: false, entries: [] },
    areas: {
      state: "no_facet",
      facet_key: null,
      facet_label: null,
      groups: [],
      untagged_task_count: 0,
      role_fallback: [
        { executor_role: "accessible-interaction-engineer", task_count: 1, task_ids: ["t1"] },
        { executor_role: "frontend-integration-engineer", task_count: 1, task_ids: ["t2"] },
      ],
    },
    kpi: { state: "not_recording", not_recording_reason: "no_rows_for_plan", enable_setting: null, spend: null },
    spend: {
      scope: "machine",
      state: "recording",
      not_recording_reason: null,
      enable_setting: null,
      measured_tokens: 3_867_500,
      turns: 3751,
      providers: [
        {
          provider: "claude",
          label: "Claude Code",
          allowance: {
            captured_at: NOW,
            session_used_pct: 17,
            session_resets_at: NOW + 2 * H,
            weekly_used_pct: 64,
            weekly_resets_at: NOW + 30 * H,
            stale_reason: null,
          },
          turns: 1973,
          input_tokens: 184_000_000,
          uncached_input_tokens: 1_400_000,
          output_tokens: 195_000,
          models: [
            {
              model: "claude-haiku-4-5",
              turns: 1973,
              input_tokens: 184_000_000,
              uncached_input_tokens: 1_400_000,
              output_tokens: 195_000,
              token_share_pct: 36,
            },
          ],
        },
        {
          provider: "codex",
          label: "Codex",
          allowance: {
            captured_at: NOW,
            session_used_pct: null,
            session_resets_at: null,
            weekly_used_pct: 6,
            weekly_resets_at: NOW + 150 * H,
            stale_reason: null,
          },
          turns: 1778,
          input_tokens: 119_077_500,
          uncached_input_tokens: 2_467_500,
          output_tokens: 182_005,
          models: [
            {
              model: null,
              turns: 1777,
              input_tokens: 119_000_000,
              uncached_input_tokens: 2_400_000,
              output_tokens: 182_000,
              token_share_pct: 62,
            },
            {
              model: "gpt-5.6-luna",
              turns: 1,
              input_tokens: 77_500,
              uncached_input_tokens: 67_500,
              output_tokens: 5,
              token_share_pct: 1.7,
            },
          ],
        },
      ],
    },
  };
}

function ownerBoardState(): PlanBoardState {
  return {
    plan_id: FIXTURE_PLAN.id,
    task_counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 4, done: 0, cancelled: 0, total: 4 },
    mission_control: ownerMission(),
  };
}

/** A fresh mount per width: happy-dom resolves a node's cascade once, on first read. */
async function renderAt(width: number) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () =>
    root.render(<MissionControlBoard plan={FIXTURE_PLAN} state={ownerBoardState()} now={NOW} />),
  );
  await act(async () => {
    for (const report of observers) report(width);
  });
  const card = (testId: string) => host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
  return {
    host,
    spend: card("mc-board-spend"),
    models: card("mc-board-models"),
    plan: card("mc-board-plan"),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function textLeaves(root: Element): HTMLElement[] {
  return (Array.from(root.querySelectorAll("*")) as HTMLElement[]).filter(
    (el) => el.children.length === 0 && (el.textContent ?? "").trim().length > 0,
  );
}

function textStyles(root: Element): Set<string> {
  return new Set(
    textLeaves(root).map((leaf) => {
      const computed = getComputedStyle(leaf);
      return `${computed.fontSize} w${computed.fontWeight}${computed.fontStyle === "italic" ? " italic" : ""}`;
    }),
  );
}

function modelRows(models: HTMLElement): HTMLElement[] {
  return Array.from(models.querySelectorAll('[data-testid="mc-board-spend-model"]')) as HTMLElement[];
}

describe("mission control board visual system", () => {
  for (const width of [OWNER_WIDTH, PHONE_WIDTH]) {
    const layout = width === PHONE_WIDTH ? "narrow" : "wide";

    it(`mission_control_board_puts_spend_models_and_plan_in_one_row_above_needs_you_at_${width}px`, async () => {
      const { host, unmount } = await renderAt(width);
      const board = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
      expect(board.getAttribute("data-board-layout")).toBe(layout);

      const row = host.querySelector('[data-testid="mc-board-row"]') as HTMLElement;
      expect(row.style.gridTemplateColumns).toBe(BOARD_ROW_COLUMNS[layout]);
      expect(Array.from(row.children).map((card) => card.getAttribute("data-testid"))).toEqual([
        "mc-board-spend",
        "mc-board-models",
        "mc-board-plan",
      ]);
      expect(board.lastElementChild?.getAttribute("data-testid")).toBe("mc-board-event-log");
      for (const removed of [
        '[data-testid="mc-board-progress"]',
        '[data-testid="mc-board-areas-toggle"]',
        '[data-testid="mc-board-spend-table"]',
        ".mc-board-kv-list",
      ]) {
        expect(host.querySelector(removed)).toBeNull();
      }
      await unmount();
    });

    it(`mission_control_models_keep_the_accepted_data_contract_at_${width}px`, async () => {
      const { models, unmount } = await renderAt(width);

      const rows = modelRows(models);
      expect(rows.map((row) => row.querySelector("th")?.textContent)).toEqual([UNASSIGNED_MODEL_LABEL, "haiku", "luna"]);
      expect(rows.map((row) => row.getAttribute("data-unassigned"))).toEqual(["true", "false", "false"]);
      expect(rows.map((row) => row.children[2]?.textContent)).toEqual(["62%", "36%", "1.7%"]);
      for (const row of rows) {
        expect(textLeaves(row).filter((leaf) => /%$/.test(leaf.textContent?.trim() ?? ""))).toEqual([row.children[2]]);
      }
      expect(rows.map((row) => row.children[1]?.textContent)).toEqual(["2.4M", "1.4M", "67.5K"]);
      const words = (models.textContent ?? "").toLowerCase();
      for (const banned of ["output", "total", "bytes", "·", "needs"]) {
        expect(words).not.toContain(banned);
      }
      await unmount();
    });

    it(`mission_control_spend_is_a_team_card_with_no_machine_limits_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      expect(spend.querySelector('[data-testid="mc-board-spend-empty"]')?.textContent).toBe("no data");
      const words = (spend.textContent ?? "").toLowerCase();
      for (const banned of ["machine", "limit", "week", "session"]) {
        expect(words).not.toContain(banned);
      }
      expect(spend.querySelector('[data-testid="mc-board-spend-gauge"]')).toBeNull();
      expect(spend.querySelector("table")).toBeNull();
      await unmount();
    });

    it(`mission_control_cards_use_at_most_four_text_styles_at_${width}px`, async () => {
      const { spend, models, unmount } = await renderAt(width);

      for (const card of [spend, models]) {
        const styles = textStyles(card);
        expect(styles.size).toBeGreaterThan(1);
        expect(styles.size).toBeLessThanOrEqual(MAX_CARD_TEXT_STYLES);
      }
      const names = modelRows(models).map((row) => getComputedStyle(row.querySelector("th") as HTMLElement));
      expect(names[0]!.fontStyle).not.toBe("italic");
      expect(names[0]!.fontSize).toBe(names[1]!.fontSize);
      expect(names[0]!.fontWeight).toBe(names[1]!.fontWeight);
      await unmount();
    });
  }

  it("mission_control_board_contains_every_card_in_a_ruled_panel", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    for (const testId of ["mc-board-spend", "mc-board-models", "mc-board-plan", "mc-board-event-log"]) {
      const panel = host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
      expect(panel.tagName).toBe("SECTION");
      expect(panel.classList.contains("ct-panel-card")).toBe(true);
      const head = panel.firstElementChild as HTMLElement;
      expect(head.tagName).toBe("HEADER");
      expect(panel.getAttribute("aria-labelledby")).toBe(head.querySelector("h2")?.id);
    }
    expect(rule(readRepo("src/styles/primitives.css"), ".ct-panel-card")).toContain("border: 1px solid");
    expect(rule(css, ".mc-board .mc-board-panel")).toContain("background: var(--ct-surface);");
    expect(rule(css, ".mc-board-panel-head")).toContain("border-bottom: 1px solid var(--ct-border);");
    expect(rule(css, ".mc-board-sub + .mc-board-sub")).toContain("padding-top: 0;");
    expect(rule(css, ".mc-board-sub-label")).toContain("font-size: var(--mc-text-meta);");
    expect(rule(css, ".mc-board-models-table thead th")).toContain("border-bottom: 1px solid var(--ct-border);");
    expect(rule(css, ".mc-board-models-table .mc-board-models-num")).toContain("font-variant-numeric: tabular-nums;");
    expect(rule(css, ".mc-board-models-table .mc-board-models-num")).toContain("text-align: right;");
    expect(rule(css, ".mc-board-row")).toContain("align-items: stretch;");
    expect(rule(css, ".mc-board-plan-heading")).toContain("-webkit-line-clamp: 3;");
    // The panel is ruled horizontally only, so vertical rules stay banned.
    expect(css).not.toMatch(/border-right|border-inline-end/);
    // The only motion is the value-change cue, and it opts out of reduced motion.
    const animations = css.match(/animation:\s*([^;]+);/g) ?? [];
    for (const declaration of animations) {
      expect(declaration).toMatch(/ct-number-changed|none/);
    }
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
    await unmount();
  });

  it("mission_control_board_uses_one_type_and_spacing_scale", () => {
    const css = readRepo(BOARD_CSS_PATH);
    const boardRules = cssRules(css).filter((entry) => entry.selector.includes("mc-board"));
    const sizes = new Set(
      boardRules.flatMap((entry) => Array.from(entry.body.matchAll(/font-size:\s*([^;]+);/g)).map((m) => m[1]!.trim())),
    );
    expect([...sizes].sort()).toEqual(["var(--mc-text-body)", "var(--mc-text-figure)", "var(--mc-text-meta)"]);
    const gaps = cssRules(css)
      .filter((entry) => /mc-board-(spend|models|plan|panel|sub|needs|status|pair)/.test(entry.selector))
      .flatMap((entry) => Array.from(entry.body.matchAll(/(?:^|\s)(?:gap|row-gap|column-gap|padding[\w-]*|margin[\w-]*):\s*([^;]+);/g)))
      .map((m) => m[1]!.trim());
    for (const value of gaps) {
      expect(value).toMatch(/^((0|auto|var\(--mc-(space-[1-4]|gap)\))\s*)+$/);
    }
    for (const entry of cssRules(css).filter((e) => /mc-board-(spend|models)/.test(e.selector))) {
      expect(entry.body).not.toContain("overflow-wrap: anywhere");
      expect(entry.body).not.toContain("font-style: italic");
    }
  });

  it("mission_control_plan_card_reads_by_goal_or_title_then_task_statuses", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, plan, unmount } = await renderAt(OWNER_WIDTH);

    expect(plan.querySelector('[data-testid="mc-board-plan-heading"]')?.textContent).toBe(FIXTURE_PLAN.title);
    expect(plan.querySelector('[data-testid="mc-board-tasks-amount"]')?.textContent).toBe("4");
    const statuses = Array.from(plan.querySelectorAll('[data-testid="mc-board-task-statuses"] li')) as HTMLElement[];
    expect(statuses.map((status) => status.getAttribute("data-testid"))).toEqual(["mc-board-task-status-paused"]);
    expect(getComputedStyle(statuses[0]!).whiteSpace).toBe("nowrap");
    expect(plan.querySelector('[data-testid="mc-board-quality"]')?.textContent).toBe("bounced1first try0 of 2");
    expect(host.querySelector('[data-testid="mc-board-needs-you"]')).toBeNull();
    expect(rule(css, ".mc-board-statuses")).toContain("flex-wrap: wrap;");
    for (const panel of Array.from(host.querySelectorAll(".mc-board-panel")) as HTMLElement[]) {
      expect(getComputedStyle(panel).justifyContent).not.toBe("space-between");
    }
    await unmount();
  });
});
