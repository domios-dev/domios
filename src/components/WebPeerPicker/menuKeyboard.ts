export type MenuKeyAction =
  | { kind: "focus"; index: number }
  | { kind: "close" }
  | null;

/** Roving focus for a menu whose items are already in visual order. */
export function menuKeyAction(key: string, currentIndex: number, itemCount: number): MenuKeyAction {
  if (key === "Escape") return { kind: "close" };
  if (itemCount <= 0) return null;
  const from = currentIndex < 0 || currentIndex >= itemCount ? -1 : currentIndex;
  switch (key) {
    case "ArrowDown":
      return { kind: "focus", index: (from + 1) % itemCount };
    // from === -1 means focus is outside the menu: ArrowUp enters at the last item.
    case "ArrowUp":
      return { kind: "focus", index: from <= 0 ? itemCount - 1 : from - 1 };
    case "Home":
      return { kind: "focus", index: 0 };
    case "End":
      return { kind: "focus", index: itemCount - 1 };
    default:
      return null;
  }
}
