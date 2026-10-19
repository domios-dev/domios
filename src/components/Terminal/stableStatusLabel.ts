export const STABLE_LABEL_WINDOW = 5;

export interface StableLabelState {
  shown: string;
  /** Shapes of the last label changes, oldest first. */
  recent: string[];
  latestByShape: Record<string, string>;
}

/** Digits are ticking detail (timers, counts); the rest of the text is what the label says. */
export function labelShape(label: string): string {
  return label.replace(/\d+/g, "#");
}

export function initialStableLabel(label: string): StableLabelState {
  return { shown: label, recent: [labelShape(label)], latestByShape: { [labelShape(label)]: label } };
}

/**
 * A ticking change shows at once. A new kind of text shows at once; text flipping between
 * recent kinds shows the most frequent of the last few, keeping the current one on a tie.
 */
export function nextStableLabel(state: StableLabelState, incoming: string, window = STABLE_LABEL_WINDOW): StableLabelState {
  if (incoming === state.shown) return state;
  const shape = labelShape(incoming);
  const latestByShape = { ...state.latestByShape, [shape]: incoming };
  if (shape === labelShape(state.shown)) return { ...state, shown: incoming, latestByShape };
  const seen = state.recent.includes(shape);
  const recent = [...state.recent, shape].slice(-window);
  if (!seen) return { shown: incoming, recent, latestByShape };
  const counts = new Map<string, number>();
  for (const entry of recent) counts.set(entry, (counts.get(entry) ?? 0) + 1);
  const currentShape = labelShape(state.shown);
  let winner = currentShape;
  let best = counts.get(currentShape) ?? 0;
  for (const [entry, count] of counts) {
    if (count > best) {
      winner = entry;
      best = count;
    }
  }
  return { shown: latestByShape[winner] ?? incoming, recent, latestByShape };
}
