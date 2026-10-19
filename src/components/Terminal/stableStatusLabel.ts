export const STABLE_LABEL_WINDOW = 5;
/** How many more recent appearances a returning text needs before it replaces the shown one. */
export const STABLE_LABEL_MARGIN = 2;

export interface StableLabelState {
  shown: string;
  /** Shapes of the last label changes, oldest first. */
  recent: string[];
  latestByShape: Record<string, string>;
}

/** Parenthetical detail and digits are ticking detail; the leading text is what the label says. */
export function labelShape(label: string): string {
  return label.replace(/\s*\(.*$/, "").replace(/\d+/g, "#").trim();
}

export function initialStableLabel(label: string): StableLabelState {
  return { shown: label, recent: [labelShape(label)], latestByShape: { [labelShape(label)]: label } };
}

/**
 * A ticking change shows at once, and so does a kind of text not seen lately. A recent kind that
 * comes back replaces the shown one only once it clearly dominates the last few changes, so text
 * flipping between two kinds settles on whichever is showing.
 */
export function nextStableLabel(
  state: StableLabelState,
  incoming: string,
  { window = STABLE_LABEL_WINDOW, placeholder = false }: { window?: number; placeholder?: boolean } = {},
): StableLabelState {
  if (incoming === state.shown) return state;
  if (placeholder && state.shown) return state;
  const shape = labelShape(incoming);
  const latestByShape = { ...state.latestByShape, [shape]: incoming };
  const currentShape = labelShape(state.shown);
  const seen = state.recent.includes(shape);
  const recent = [...state.recent, shape].slice(-window);
  if (shape === currentShape) return { shown: incoming, recent, latestByShape };
  if (!seen) return { shown: incoming, recent, latestByShape };
  const count = (target: string) => recent.filter((entry) => entry === target).length;
  const dominates = count(shape) >= count(currentShape) + STABLE_LABEL_MARGIN;
  return { shown: dominates ? incoming : state.shown, recent, latestByShape };
}
