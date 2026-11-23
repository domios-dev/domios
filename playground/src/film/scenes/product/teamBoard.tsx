import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { Task } from "../../../../../src/components/Canvas/types";
import { BOARD_COLUMN_DEFS, buildBoardPlanModel, type BoardColumnKey, type BoardTaskCardModel } from "../../../../../src/components/Canvas/planView/boardModel";
import { BoardTaskCard } from "../../../../../src/components/Canvas/planView/BoardTaskCard";
import { FILM_NOW } from "../../fixtures/clock";
import { PLAN, PLAN_TABS, SECTIONS } from "../../fixtures/plan";
import { easeInOut, easeOut, lerp, range } from "../../engine";

type Status = Task["status"];

/** One change on the board: a task moves to `to` (or is added with `add`), done by the member titled `by`. */
export interface BoardMove {
  at: number;
  task: string;
  to: Status;
  by: string;
  add?: Task;
  /** Fields the move changes besides the status, such as the worker it is assigned to. */
  patch?: Partial<Task>;
}

export interface BoardGeometry {
  x: number;
  y: number;
  column: number;
  gap: number;
  height: number;
  slot: number;
}

const FLIGHT = 0.7;
const HEAD = 34;
const COLUMNS = BOARD_COLUMN_DEFS.map((def) => def.key);
const columnOf = (status: Status): BoardColumnKey => BOARD_COLUMN_DEFS.find((def) => def.statuses.includes(status))?.key ?? "pending";

interface BoardState {
  tasks: Task[];
  order: Record<BoardColumnKey, string[]>;
  cards: Map<string, BoardTaskCardModel>;
}

function snapshot(tasks: Task[], order: Record<BoardColumnKey, string[]>): BoardState {
  const model = buildBoardPlanModel({ ...PLAN, tasks }, PLAN_TABS, SECTIONS, FILM_NOW);
  const cards = new Map(model.columns.flatMap((column) => column.cards.map((card) => [card.task.id, card] as const)));
  return { tasks, order, cards };
}

/** Every board state the moves pass through; a moved or added card lands at the top of its column, newest first. */
export function boardStates(moves: BoardMove[]): BoardState[] {
  const order = Object.fromEntries(COLUMNS.map((key) => [key, PLAN.tasks.filter((task) => columnOf(task.status) === key).map((task) => task.id)])) as Record<BoardColumnKey, string[]>;
  const states = [snapshot(PLAN.tasks, order)];
  for (const move of moves) {
    const prev = states[states.length - 1];
    const tasks = move.add ? [...prev.tasks, { ...move.add, status: move.to }] : prev.tasks.map((task) => (task.id === move.task ? { ...task, ...move.patch, status: move.to } : task));
    const next = Object.fromEntries(COLUMNS.map((key) => [key, prev.order[key].filter((id) => id !== move.task)])) as Record<BoardColumnKey, string[]>;
    next[columnOf(move.to)] = [move.task, ...next[columnOf(move.to)]];
    states.push(snapshot(tasks, next));
  }
  return states;
}

function slotOf(state: BoardState, id: string): [number, number] | null {
  for (const [c, key] of COLUMNS.entries()) {
    const slot = state.order[key].indexOf(id);
    if (slot >= 0) return [c, slot];
  }
  return null;
}

const GAP = 8;
const variantOf = (card: BoardTaskCardModel) => `${card.task.id}|${card.task.status}|${card.task.assigned_tab_id ?? ""}`;

/** The plan's kanban as the app draws it, with each card gliding to its new column as the team moves it. */
export function TeamBoard({ t, moves, states, geometry, style }: { t: number; moves: BoardMove[]; states: BoardState[]; geometry: BoardGeometry; style?: CSSProperties }) {
  const { x, y, column, gap, height, slot } = geometry;
  const started = moves.filter((move) => move.at <= t).length;
  const move = started > 0 ? moves[started - 1] : null;
  const k = move ? range(t, move.at, move.at + FLIGHT, easeInOut) : 1;
  const prev = states[Math.max(0, started - 1)];
  const next = states[started];
  const shown = k >= 0.5 ? next : prev;
  // Cards stack by their own drawn height, measured from a hidden copy of every version the moves pass through; `slot` covers the first frame.
  const variants = useMemo(() => [...new Map(states.flatMap((state) => [...state.cards.values()].map((card) => [variantOf(card), card] as const))).values()], [states]);
  const measure = useRef<HTMLDivElement>(null);
  const [heights, setHeights] = useState<Record<string, number>>({});
  useLayoutEffect(() => {
    const found: Record<string, number> = {};
    for (const el of measure.current?.children ?? []) found[(el as HTMLElement).dataset.variant!] = (el as HTMLElement).offsetHeight;
    if (Object.entries(found).some(([key, h]) => heights[key] !== h)) setHeights(found);
  });
  const heightOf = (state: BoardState, id: string) => {
    const card = state.cards.get(id);
    return (card && heights[variantOf(card)]) || slot - GAP;
  };
  const at = (state: BoardState, [c, s]: [number, number]): [number, number] => {
    const above = state.order[COLUMNS[c]].slice(0, s).reduce((sum, id) => sum + heightOf(state, id) + GAP, 0);
    return [x + c * (column + gap) + 6, y + HEAD + 8 + above];
  };
  const ids = [...new Set([...prev.tasks, ...next.tasks].map((task) => task.id))];

  return (
    <div style={style}>
      <div ref={measure} aria-hidden style={{ position: "absolute", left: 0, top: 0, visibility: "hidden", pointerEvents: "none" }}>
        {variants.map((card) => (
          <div key={variantOf(card)} data-variant={variantOf(card)} style={{ position: "absolute", width: column - 12 }}>
            <BoardTaskCard card={card} planId={PLAN.id} />
          </div>
        ))}
      </div>
      {COLUMNS.map((key, c) => {
        const def = BOARD_COLUMN_DEFS[c];
        return (
          <div key={key} className="plan-board-column" data-column={key} style={{ position: "absolute", left: x + c * (column + gap), top: y, width: column, height }}>
            <header className="plan-board-column-head">
              <span>{def.label}</span>
              <span className="plan-board-column-count">{shown.order[key].length}</span>
            </header>
          </div>
        );
      })}
      {ids.map((id) => {
        const from = slotOf(prev, id);
        const to = slotOf(next, id);
        const moving = move?.task === id && k < 1;
        const card = (k >= 0.5 ? next : prev).cards.get(id) ?? next.cards.get(id);
        if (!card || (!from && !to)) return null;
        const [x0, y0] = from ? at(prev, from) : to ? [at(next, to)[0], at(next, to)[1] - 60] : [0, 0];
        const [x1, y1] = to ? at(next, to) : [x0, y0];
        const kk = move ? k : 1;
        const cx = lerp(x0, x1, kk);
        const cy = lerp(y0, y1, kk);
        const appear = from ? 1 : range(t, move?.at ?? 0, (move?.at ?? 0) + 0.4, easeOut);
        const lift = moving ? Math.sin(Math.PI * k) : 0;
        const tag = move?.task === id ? 1 - range(t, move.at + FLIGHT + 0.5, move.at + FLIGHT + 0.9) : 0;
        return (
          <div
            key={id}
            style={{
              position: "absolute",
              left: cx,
              top: cy,
              width: column - 12,
              zIndex: moving ? 3 : 1,
              opacity: appear,
              transform: `scale(${1 + 0.05 * lift})`,
              filter: lift > 0 ? `drop-shadow(0 ${14 * lift}px ${22 * lift}px rgb(0 0 0 / ${0.22 * lift}))` : undefined,
            }}
          >
            <BoardTaskCard card={card} planId={PLAN.id} />
            {tag > 0 && move && (
              <span style={{ position: "absolute", right: 8, top: -12, padding: "2px 9px", borderRadius: 999, background: "var(--ct-accent)", color: "var(--ct-on-accent, #fff)", fontSize: 12, fontWeight: 600, opacity: tag, boxShadow: "var(--ct-shadow-md)" }}>
                {move.by}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
