import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { MissionControlBoard } from "./MissionControlBoard";
import {
  ROLE_FALLBACK_TITLE,
  SPEND_SCOPE_CAPTION,
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

    it(`mission_control_spend_is_one_table_grouped_by_provider_at_${width}px`, async () => {
      const { host, spend, unmount } = await renderAt(width);
      expect(host.querySelector('[data-testid="mission-control-board"]')?.getAttribute("data-board-layout")).toBe(layout);

      const heading = spend.querySelector("h2") as HTMLElement;
      expect(heading.textContent).toBe("Spend");
      expect(spend.getAttribute("aria-labelledby")).toBe(heading.id);
      const table = spend.querySelector("table") as HTMLTableElement;
      expect(table.getAttribute("role")).toBe("table");
      const columns = Array.from(table.querySelectorAll('thead th[scope="col"]'));
      expect(columns).toHaveLength(5);

      const groups = Array.from(table.querySelectorAll("tbody")) as HTMLElement[];
      expect(groups.map((group) => [group.getAttribute("role"), group.getAttribute("data-provider")])).toEqual([
        ["rowgroup", "claude"],
        ["rowgroup", "codex"],
      ]);
      for (const group of groups) {
        const [head, ...rows] = Array.from(group.children) as HTMLElement[];
        expect(head!.querySelector("th")?.getAttribute("scope")).toBe("rowgroup");
        expect(head!.querySelectorAll('[data-testid="mc-board-spend-gauge"]').length).toBeGreaterThan(0);
        expect(rows.length).toBeGreaterThan(0);
        for (const row of rows) {
          expect(row.getAttribute("role")).toBe("row");
          const cells = Array.from(row.children);
          expect(cells).toHaveLength(1 + columns.length);
          expect([cells[0]!.tagName, cells[0]!.getAttribute("scope")]).toEqual(["TH", "row"]);
          expect(cells.slice(1).every((cell) => cell.tagName === "TD" && cell.getAttribute("role") === "cell")).toBe(true);
        }
      }
      expect(Array.from(spend.querySelectorAll(".mc-board-spend-provider-name")).map((name) => name.textContent)).toEqual([
        "Claude Code",
        "Codex",
      ]);
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
        expect(textLeaves(row).filter((leaf) => /%$/.test(leaf.textContent?.trim() ?? ""))).toEqual([row.children[1]]);
      }
      const figure = (row: Element, key: string) => row.querySelector(`[data-figure="${key}"]`)?.textContent;
      expect(figure(rows[1]!, "turns")).toBe("1,777");
      const [claude, codex] = Array.from(spend.querySelectorAll('[data-testid="mc-board-spend-group"]'));
      expect(figure(claude!, "fromCache")).toBe("183M");
      expect(figure(codex!, "newInput")).toBe("2.5M");
      expect(figure(codex!, "fromCache")).toBe("117M");
      expect(
        Array.from(spend.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).map((gauge) =>
          gauge.getAttribute("data-window-key"),
        ),
      ).toEqual(["claude-weekly", "claude-session", "codex-weekly"]);
      const words = (spend.textContent ?? "").toLowerCase();
      for (const banned of ["output", "total", "bytes", "·", "needs", "took part"]) {
        expect(words).not.toContain(banned);
      }
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

      const names = modelRows(spend).map((row) =>
        getComputedStyle(row.querySelector(".mc-board-spend-model-name") as HTMLElement),
      );
      const nameSizes = names.map((style) => px(style.fontSize));
      for (const header of Array.from(spend.querySelectorAll(".mc-board-spend-provider-name")) as HTMLElement[]) {
        expect(px(getComputedStyle(header).fontSize)).toBeGreaterThanOrEqual(Math.max(...nameSizes));
      }

      const [known, unassigned] = names;
      expect(unassigned!.fontStyle).not.toBe("italic");
      expect(unassigned!.fontSize).toBe(known!.fontSize);
      expect(unassigned!.fontWeight).toBe(known!.fontWeight);
      await unmount();
    });

    it(`mission_control_spend_rows_share_one_track_template_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      const header = spend.querySelector('[data-testid="mc-board-spend-columns"]') as HTMLElement;
      const groupRows = Array.from(spend.querySelectorAll('[data-testid="mc-board-spend-group"]')) as HTMLElement[];
      const computed = [header, ...groupRows, ...modelRows(spend)].map((row) => getComputedStyle(row));
      expect(computed.every((style) => style.display === "grid")).toBe(true);
      expect(new Set(computed.map((style) => style.gridTemplateColumns)).size).toBe(1);
      // The share is a bare text cell, so its box is its grid line on every model row.
      for (const row of modelRows(spend)) {
        expect(row.children[1]!.classList.contains("mc-board-spend-share")).toBe(true);
        expect(row.children[1]!.children).toHaveLength(0);
      }
      await unmount();
    });
  }

  it("mission_control_board_contains_every_section_in_a_ruled_panel", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    for (const testId of ["mc-board-progress", "mc-board-event-log"]) {
      const panel = host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
      expect(panel.tagName).toBe("SECTION");
      expect(panel.classList.contains("ct-panel-card")).toBe(true);
      const head = panel.firstElementChild as HTMLElement;
      expect(head.tagName).toBe("HEADER");
      expect(panel.getAttribute("aria-labelledby")).toBe(head.querySelector("h2")?.id);
    }
    expect(rule(readRepo("src/styles/globals.css"), ".ct-panel-card")).toContain("border: 1px solid");
    expect(rule(css, ".mc-board .mc-board-panel")).toContain("background: var(--ct-surface);");
    expect(rule(css, ".mc-board-panel-head")).toContain("border-bottom: 1px solid var(--ct-border);");
    expect(rule(css, ".mc-board-sub + .mc-board-sub")).toContain("border-top: 1px solid var(--ct-border-subtle);");
    expect(rule(css, ".mc-board-row")).toContain("align-items: start;");
    expect(rule(css, ".mc-board-spend-row > .mc-board-spend-num")).toContain("font-variant-numeric: tabular-nums;");
    expect(rule(css, ".mc-board-spend-row > .mc-board-spend-num")).toContain("text-align: right;");
    await unmount();
  });

  it("mission_control_board_uses_one_type_and_spacing_scale", () => {
    const css = readRepo(BOARD_CSS_PATH);
    const boardRules = cssRules(css).filter((entry) => entry.selector.includes("mc-board"));
    const sizes = new Set(
      boardRules.flatMap((entry) => Array.from(entry.body.matchAll(/font-size:\s*([^;]+);/g)).map((m) => m[1]!.trim())),
    );
    expect([...sizes].sort()).toEqual(["var(--mc-text-body)", "var(--mc-text-figure)", "var(--mc-text-meta)"]);
    const spendGaps = cssRules(css)
      .filter((entry) => /mc-board-(spend|card|panel|sub|needs|status|pair|kv)/.test(entry.selector))
      .flatMap((entry) => Array.from(entry.body.matchAll(/(?:^|\s)(?:gap|row-gap|column-gap|padding[\w-]*|margin[\w-]*):\s*([^;]+);/g)))
      .map((m) => m[1]!.trim());
    for (const value of spendGaps) {
      expect(value).toMatch(/^((0|auto|var\(--mc-(space-[1-4]|gap|card-pad)\))\s*)+$/);
    }
    for (const entry of cssRules(css).filter((e) => e.selector.includes("mc-board-spend"))) {
      expect(entry.body).not.toContain("overflow-wrap: anywhere");
      expect(entry.body).not.toContain("font-style: italic");
    }
  });

  it("mission_control_board_progress_names_only_statuses_that_hold_tasks", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    const statuses = Array.from(host.querySelectorAll('[data-testid="mc-board-task-statuses"] li')) as HTMLElement[];
    expect(statuses.map((status) => status.getAttribute("data-testid"))).toEqual(["mc-board-task-status-paused"]);
    expect(getComputedStyle(statuses[0]!).whiteSpace).toBe("nowrap");
    expect(host.querySelector('[data-testid="mc-board-task-statuses"]')?.textContent).toBe("4paused");
    expect(host.querySelector('[data-testid="mc-board-needs-you"]')).toBeNull();
    expect(rule(css, ".mc-board-statuses")).toContain("flex-wrap: wrap;");
    expect(rule(css, ".mc-board-fact")).toContain("white-space: nowrap;");
    for (const panel of Array.from(host.querySelectorAll(".mc-board-panel")) as HTMLElement[]) {
      expect(getComputedStyle(panel).justifyContent).not.toBe("space-between");
    }
    await unmount();
  });

  it("mission_control_board_keeps_role_counts_beside_their_role_names", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    const list = host.querySelector(".mc-board-kv-list") as HTMLElement;
    expect(list.tagName).toBe("DL");
    expect(getComputedStyle(list).display).toBe("grid");
    expect(host.querySelector('[data-testid="mc-board-areas"] h3')?.textContent).toBe(ROLE_FALLBACK_TITLE);
    const rows = Array.from(host.querySelectorAll('[data-testid="mc-board-area-row"]')) as HTMLElement[];
    expect(rows).toHaveLength(4);
    expect(rows.every((row) => getComputedStyle(row).display === "contents")).toBe(true);
    expect(rule(css, ".mc-board-kv-list")).toContain("grid-template-columns: minmax(0, 1fr) auto;");
    for (const row of rows) {
      expect(Array.from(row.children).map((cell) => [cell.tagName, cell.className])).toEqual([
        ["DT", "mc-board-kv-key"],
        ["DD", "mc-board-kv-value"],
      ]);
    }
    await unmount();
  });

  it("mission_control_spend_puts_names_above_figures_in_a_narrow_panel", async () => {
    const css = readRepo(BOARD_CSS_PATH);
    const { spend, unmount } = await renderAt(PHONE_WIDTH);

    const narrow = css.slice(css.indexOf("@container mc-spend (max-width: 520px)"));
    expect(narrow.length).toBeLessThan(css.length);
    expect(rule(narrow, ".mc-board-spend-row")).toContain("grid-template-columns: repeat(5, minmax(0, 1fr));");
    expect(rule(narrow, ".mc-board-spend-row > .mc-board-spend-name")).toContain("grid-column: 1 / -1;");
    expect(rule(narrow, ".mc-board-spend-label-short")).toContain("display: inline;");
    expect(rule(css, ".mc-board-spend")).toContain("container: mc-spend / inline-size;");
    expect(["auto", "scroll"]).not.toContain(getComputedStyle(spend).overflowX);
    for (const row of modelRows(spend)) {
      expect(row.querySelector("th")?.textContent?.trim().length).toBeGreaterThan(0);
      expect(row.querySelectorAll("[data-figure]")).toHaveLength(4);
    }
    await unmount();
  });
});
