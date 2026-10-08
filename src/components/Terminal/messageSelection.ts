/** Chat message selection: `base` is what an extend run started from, so Shift+arrows can grow and shrink a range. */
export interface MessageSelection {
  selected: readonly string[];
  base: readonly string[];
  anchor: string | null;
  cursor: string | null;
}

export type MessageSelectionAction =
  | { type: "toggle"; id: string }
  | { type: "extend"; id: string; order: readonly string[] }
  | { type: "move"; delta: 1 | -1; order: readonly string[]; extend: boolean }
  | { type: "focus"; id: string }
  | { type: "prune"; order: readonly string[] }
  | { type: "clear" };

export const EMPTY_SELECTION: MessageSelection = Object.freeze({ selected: [], base: [], anchor: null, cursor: null });

function range(order: readonly string[], from: string, to: string): string[] {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return [to];
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}

function union(order: readonly string[], ...lists: ReadonlyArray<readonly string[]>): string[] {
  const wanted = new Set(lists.flat());
  return order.filter((id) => wanted.has(id));
}

function extendTo(state: MessageSelection, id: string, order: readonly string[]): MessageSelection {
  const anchor = state.anchor && order.includes(state.anchor) ? state.anchor : id;
  return { ...state, selected: union(order, state.base, range(order, anchor, id)), anchor, cursor: id };
}

export function reduceMessageSelection(state: MessageSelection, action: MessageSelectionAction): MessageSelection {
  switch (action.type) {
    case "toggle": {
      const selected = state.selected.includes(action.id)
        ? state.selected.filter((id) => id !== action.id)
        : [...state.selected, action.id];
      if (selected.length === 0) return EMPTY_SELECTION;
      return { selected, base: selected, anchor: action.id, cursor: action.id };
    }
    case "extend":
      return extendTo(state, action.id, action.order);
    case "move": {
      if (action.order.length === 0) return state;
      const at = state.cursor ? action.order.indexOf(state.cursor) : -1;
      const next = at < 0
        ? action.order[action.delta > 0 ? 0 : action.order.length - 1]
        : action.order[Math.min(action.order.length - 1, Math.max(0, at + action.delta))];
      if (!action.extend) return { ...state, cursor: next };
      return extendTo(state.anchor ? state : { ...state, anchor: state.cursor ?? next }, next, action.order);
    }
    case "focus":
      return state.cursor === action.id ? state : { ...state, cursor: action.id };
    case "prune": {
      const live = new Set(action.order);
      if (state.selected.every((id) => live.has(id)) && (!state.cursor || live.has(state.cursor))) return state;
      const selected = state.selected.filter((id) => live.has(id));
      if (selected.length === 0) return EMPTY_SELECTION;
      return {
        selected,
        base: state.base.filter((id) => live.has(id)),
        anchor: state.anchor && live.has(state.anchor) ? state.anchor : null,
        cursor: state.cursor && live.has(state.cursor) ? state.cursor : null,
      };
    }
    case "clear":
      return EMPTY_SELECTION;
  }
}

/** Selected ids in transcript order, whatever order they were picked in. */
export function orderedSelection(state: MessageSelection, order: readonly string[]): string[] {
  return union(order, state.selected);
}

export function selectionActive(state: MessageSelection): boolean {
  return state.selected.length > 0;
}
