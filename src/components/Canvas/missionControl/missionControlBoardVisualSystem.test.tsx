import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { MissionControlBoard } from "./MissionControlBoard";
import {
  BOARD_CARD_COLUMNS,
  BOARD_LEAD_FRACTION,
  BOARD_TRAILING_TRACKS,
  SPEND_FIGURE_COLUMNS,
  SPEND_MODEL_COLUMN_LABEL,
  SPEND_SCOPE_CAPTION,
  SPEND_SHARE_COLUMN_LABEL,
  UNASSIGNED_MODEL_LABEL,
} from "./missionControlBoardModel";
import type { MissionControlState, PlanBoardState } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const NOW = 1_700_000_000_000;
const OWNER_WIDTH = 1456;
const PHONE_WIDTH = 390;
const MAX_SPEND_TEXT_STYLES = 4;

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
  style.textContent = [
    readRepo("src/styles/globals.css"),
    readRepo("src/components/Usage/styles.css"),
    readRepo(BOARD_CSS_PATH),
  ].join("\n");
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
        { executor_role: "game-logic-engineer", task_count: 1, task_ids: ["t3"] },
        { executor_role: "game-rendering-engineer", task_count: 1, task_ids: ["t4"] },
      ],
    },
    kpi: { state: "not_recording", not_recording_reason: "no_rows_for_plan", enable_setting: null, spend: null },
    spend: {
      scope: "machine",
      state: "recording",
      not_recording_reason: null,
      enable_setting: null,
      measured_tokens: 700_000_000,
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
              token_share_pct: 61,
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
              token_share_pct: 39,
            },
            {
              model: "gpt-5.6-luna",
              turns: 1,
              input_tokens: 77_500,
              uncached_input_tokens: 67_500,
              output_tokens: 5,
              token_share_pct: 0,
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
  const spend = host.querySelector('[data-testid="mc-board-spend"]') as HTMLElement;
  return {
    host,
    spend,
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

function px(value: string): number {
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed)) throw new Error(`not a resolved length: "${value}"`);
  return parsed;
}

function modelRows(spend: HTMLElement): HTMLElement[] {
  return Array.from(spend.querySelectorAll('[data-testid="mc-board-spend-model"]')) as HTMLElement[];
}

describe("mission control board visual system", () => {
  for (const width of [OWNER_WIDTH, PHONE_WIDTH]) {
    const layout = width === PHONE_WIDTH ? "narrow" : "wide";

    it(`mission_control_spend_is_a_semantic_table_at_${width}px`, async () => {
      const { host, spend, unmount } = await renderAt(width);
      expect(host.querySelector('[data-testid="mission-control-board"]')?.getAttribute("data-board-layout")).toBe(layout);

      const table = spend.querySelector("table") as HTMLTableElement;
      expect(table.getAttribute("role")).toBe("table");
      const heading = spend.querySelector("h2") as HTMLElement;
      expect(heading.textContent).toBe("Spend");
      expect(table.getAttribute("aria-labelledby")).toBe(heading.id);

      const columns = Array.from(table.querySelectorAll('thead [role="columnheader"]'));
      expect(columns.map((th) => [th.tagName, th.getAttribute("scope"), th.textContent])).toEqual(
        [SPEND_MODEL_COLUMN_LABEL, SPEND_SHARE_COLUMN_LABEL, ...SPEND_FIGURE_COLUMNS.map((c) => c.label)].map(
          (label) => ["TH", "col", label],
        ),
      );

      const groups = Array.from(table.children).filter((child) => child.tagName === "TBODY") as HTMLElement[];
      expect(groups.map((group) => [group.getAttribute("role"), group.getAttribute("data-provider")])).toEqual([
        ["rowgroup", "claude"],
        ["rowgroup", "codex"],
      ]);
      for (const group of groups) {
        const [head, ...rows] = Array.from(group.children) as HTMLElement[];
        const providerHeader = head!.querySelector("th") as HTMLElement;
        expect(providerHeader.getAttribute("scope")).toBe("rowgroup");
        expect(providerHeader.children).toHaveLength(0);
        // Allowance readings belong to the group header row, above every model row.
        expect(head!.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(2);
        expect(rows.length).toBeGreaterThan(0);
        for (const row of rows) {
          expect(row.getAttribute("role")).toBe("row");
          expect(row.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(0);
          const cells = Array.from(row.children);
          expect(cells).toHaveLength(columns.length);
          expect([cells[0]!.tagName, cells[0]!.getAttribute("scope")]).toEqual(["TH", "row"]);
          expect(cells.slice(1).every((cell) => cell.tagName === "TD" && cell.getAttribute("role") === "cell")).toBe(true);
        }
      }
      expect(groups[0]!.querySelector("th")?.textContent).toBe("Claude Code");
      expect(groups[1]!.querySelector("th")?.textContent).toBe("Codex");
      await unmount();
    });

    it(`mission_control_spend_keeps_the_accepted_data_contract_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      expect(spend.querySelector('[data-testid="mc-board-spend-scope"]')?.textContent).toBe(SPEND_SCOPE_CAPTION);
      expect(spend.querySelectorAll("p")).toHaveLength(0);
      const rows = modelRows(spend);
      expect(rows.map((row) => row.querySelector("th")?.textContent)).toEqual(["haiku", UNASSIGNED_MODEL_LABEL, "luna"]);
      expect(rows.map((row) => row.getAttribute("data-unassigned"))).toEqual(["false", "true", "false"]);
      expect(rows.map((row) => row.children[1]?.textContent)).toEqual(["61%", "39%", "0%"]);
      for (const row of rows) {
        const percentCells = Array.from(row.children).filter((cell) => /%$/.test(cell.textContent?.trim() ?? ""));
        expect(percentCells).toHaveLength(1);
        expect(percentCells[0]).toBe(row.children[1]);
      }
      expect(rows[1]!.textContent).toContain("1,777");
      expect(spend.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(4);
      await unmount();
    });

    it(`mission_control_spend_uses_at_most_four_text_styles_with_headers_no_smaller_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      const styles = new Set(
        textLeaves(spend).map((leaf) => {
          const computed = getComputedStyle(leaf);
          return `${computed.fontSize} w${computed.fontWeight}${computed.fontStyle === "italic" ? " italic" : ""}`;
        }),
      );
      expect(styles.size).toBeGreaterThan(1);
      expect(styles.size).toBeLessThanOrEqual(MAX_SPEND_TEXT_STYLES);

      const names = modelRows(spend).map((row) => getComputedStyle(row.querySelector("th") as HTMLElement));
      const nameSizes = names.map((style) => px(style.fontSize));
      for (const header of Array.from(spend.querySelectorAll('th[scope="rowgroup"]')) as HTMLElement[]) {
        expect(px(getComputedStyle(header).fontSize)).toBeGreaterThanOrEqual(Math.max(...nameSizes));
      }

      const [known, unassigned] = names;
      expect(unassigned!.fontStyle).not.toBe("italic");
      expect(unassigned!.fontSize).toBe(known!.fontSize);
      expect(unassigned!.fontWeight).toBe(known!.fontWeight);
      await unmount();
    });

    it(`mission_control_spend_rows_share_one_fixed_track_template_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      const header = spend.querySelector('[data-testid="mc-board-spend-columns"]') as HTMLElement;
      const rows = [header, ...modelRows(spend)];
      const computed = rows.map((row) => getComputedStyle(row));
      expect(computed.every((style) => style.display === "grid")).toBe(true);
      const templates = new Set(computed.map((style) => style.gridTemplateColumns));
      expect(templates.size).toBe(1);
      // The share cell is a bare text cell, so its box (the audit's x) is its grid line.
      for (const row of modelRows(spend)) {
        expect(row.children[1]!.classList.contains("mc-board-spend-model-share")).toBe(true);
        expect(row.children[1]!.children).toHaveLength(0);
      }
      await unmount();
    });
  }

  it("mission_control_board_derives_the_spend_lead_column_from_the_card_grid", () => {
    const css = readRepo(BOARD_CSS_PATH);
    const tracks = BOARD_LEAD_FRACTION + BOARD_TRAILING_TRACKS;

    expect(BOARD_CARD_COLUMNS.wide).toBe(
      `minmax(0, ${BOARD_LEAD_FRACTION}fr) repeat(${BOARD_TRAILING_TRACKS}, minmax(0, 1fr))`,
    );
    const board = rule(css, ".mc-board");
    expect(board).toContain(
      `--mc-lead: calc((100% + 2 * var(--mc-card-inset) - ${BOARD_TRAILING_TRACKS} * var(--mc-gap)) * ${BOARD_LEAD_FRACTION} / ${tracks} - var(--mc-card-inset));`,
    );
    expect(board).toContain("--mc-card-inset: calc(var(--mc-card-pad) + 1px);");
    expect(rule(readRepo("src/styles/globals.css"), ".ct-panel-card")).toContain("border: 1px solid");
    expect(rule(css, ".mc-board-card")).toContain("padding: var(--mc-card-pad);");
    expect(rule(css, ".mc-board-card-grid, .mc-board-spend-window-grid")).toContain("gap: var(--mc-gap);");
    expect(rule(css, ".mc-board-spend-row")).toContain(
      `grid-template-columns: var(--mc-lead) repeat(${1 + SPEND_FIGURE_COLUMNS.length}, minmax(0, 1fr));`,
    );
    expect(rule(css, ".mc-board-spend-row > .mc-board-spend-num")).toContain("font-variant-numeric: tabular-nums;");
    expect(rule(css, ".mc-board-spend-row > .mc-board-spend-num")).toContain("text-align: right;");
  });

  it("mission_control_board_uses_one_type_and_spacing_scale", () => {
    const css = readRepo(BOARD_CSS_PATH);
    const boardRules = cssRules(css).filter((entry) => entry.selector.includes("mc-board"));
    const sizes = new Set(
      boardRules.flatMap((entry) => Array.from(entry.body.matchAll(/font-size:\s*([^;]+);/g)).map((m) => m[1]!.trim())),
    );
    expect([...sizes].sort()).toEqual(["var(--mc-text-body)", "var(--mc-text-figure)", "var(--mc-text-meta)"]);
    const spendGaps = cssRules(css)
      .filter((entry) => /mc-board-(spend|card|reading|status|kv)/.test(entry.selector))
      .flatMap((entry) => Array.from(entry.body.matchAll(/(?:^|\s)(?:gap|row-gap|column-gap|padding[\w-]*|margin[\w-]*):\s*([^;]+);/g)))
      .map((m) => m[1]!.trim());
    for (const value of spendGaps) {
      expect(value).toMatch(/^(0|(var\(--mc-(space-[1-4]|gap|card-pad)\)\s*)+|0 var\(--mc-space-[1-4]\))$/);
    }
    for (const entry of cssRules(css).filter((e) => e.selector.includes("mc-board-spend"))) {
      expect(entry.body).not.toContain("overflow-wrap: anywhere");
      expect(entry.body).not.toContain("font-style: italic");
    }
  });

  it("mission_control_board_top_cards_anchor_their_reading_instead_of_inheriting_dead_space", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    const cards = Array.from(host.querySelectorAll('[data-testid="mc-board-card-grid"] > .mc-board-card')) as HTMLElement[];
    expect(cards).toHaveLength(8);
    for (const card of cards) {
      expect(card.classList.contains("mc-board-figure-card")).toBe(true);
      expect(Array.from(card.children).map((child) => child.tagName === "H2" || child.classList.contains("mc-board-reading"))).toEqual([true, true]);
    }
    const tasksReading = host.querySelector('[data-testid="mc-board-tasks-reading"]') as HTMLElement;
    expect(tasksReading.querySelector('[data-testid="mc-board-task-statuses"]')).not.toBeNull();
    expect(getComputedStyle(tasksReading).display).toBe("grid");
    for (const card of cards) {
      expect(getComputedStyle(card).justifyContent).not.toBe("space-between");
    }
    expect(rule(css, ".mc-board-card-tasks .mc-board-reading")).toContain(
      "grid-template-columns: max-content minmax(0, 1fr);",
    );
    expect(rule(css, ".mc-board-card-tasks .mc-board-reading")).toContain("align-items: start;");
    const statusList = rule(css, ".mc-board-status-list");
    expect(statusList).toContain("flex-wrap: wrap;");
    expect(rule(css, ".mc-board-status")).toContain("white-space: nowrap;");
    const statuses = Array.from(host.querySelectorAll('[data-testid^="mc-board-task-status-"][role="listitem"]'));
    expect(statuses.map((status) => status.getAttribute("data-zero"))).toEqual([
      "true",
      "true",
      "true",
      "true",
      "false",
      "true",
      "true",
    ]);
    expect(
      rule(
        css,
        '.mc-board[data-board-layout="narrow"] .mc-board-card-grid > .mc-board-card-tasks, .mc-board[data-board-layout="narrow"] .mc-board-card-grid > .mc-board-card:nth-child(even):last-child',
      ),
    ).toContain("grid-column: 1 / -1;");
    await unmount();
  });

  it("mission_control_board_keeps_role_counts_beside_their_role_names", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    const list = host.querySelector(".mc-board-kv-list") as HTMLElement;
    expect(getComputedStyle(list).display).toBe("grid");
    const rows = Array.from(host.querySelectorAll('[data-testid="mc-board-area-row"]')) as HTMLElement[];
    expect(rows).toHaveLength(4);
    expect(rows.every((row) => getComputedStyle(row).display === "contents")).toBe(true);
    const template = rule(css, ".mc-board-kv-list");
    expect(template).toContain("grid-template-columns: minmax(0, max-content) max-content max-content;");
    expect(template).toContain("justify-content: start;");
    expect(template).not.toContain("1fr");
    const wide = rule(css, '.mc-board[data-board-layout="wide"] .mc-board-kv-list');
    expect(wide).toContain("grid-template-columns: repeat(2, minmax(0, max-content) max-content max-content);");
    expect(wide).not.toContain("1fr");
    expect(rule(css, '.mc-board[data-board-layout="wide"] .mc-board-kv:nth-child(even) > .mc-board-kv-key')).toContain(
      "padding-left: var(--mc-space-4);",
    );
    for (const row of rows) {
      expect(Array.from(row.children).map((cell) => cell.className)).toEqual([
        "mc-board-kv-key",
        "mc-board-kv-value",
        "mc-board-kv-share",
      ]);
    }
    await unmount();
  });

  it("mission_control_spend_composes_a_readable_phone_table_without_horizontal_scroll", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { spend, unmount } = await renderAt(PHONE_WIDTH);

    const narrowRow = rule(css, '.mc-board[data-board-layout="narrow"] .mc-board-spend-row');
    expect(narrowRow).toContain("grid-template-columns: repeat(4, minmax(0, 1fr));");
    expect(narrowRow).not.toMatch(/\d+px/);
    expect(rule(css, '.mc-board[data-board-layout="narrow"] .mc-board-spend-row > :first-child')).toContain(
      "grid-column: 1 / 4;",
    );
    expect(rule(css, ".mc-board-spend-row > th, .mc-board-spend-row > td")).toContain("overflow-wrap: break-word;");

    const table = spend.querySelector("table") as HTMLElement;
    for (const el of [spend, table]) {
      expect(["auto", "scroll"]).not.toContain(getComputedStyle(el).overflowX);
    }
    // Column labels stay in the accessibility tree at phone width, drawn above the figures they name.
    const header = spend.querySelector('[data-testid="mc-board-spend-columns"]') as HTMLElement;
    expect(getComputedStyle(header).display).toBe("grid");
    expect(header.querySelectorAll('[role="columnheader"]')).toHaveLength(2 + SPEND_FIGURE_COLUMNS.length);
    for (const row of modelRows(spend)) {
      expect(row.querySelector("th")?.textContent?.trim().length).toBeGreaterThan(0);
      expect(row.querySelectorAll(".mc-board-spend-figure")).toHaveLength(SPEND_FIGURE_COLUMNS.length);
    }
    await unmount();
  });
});
