export interface InstallHintBounds {
  touch: boolean;
  viewportTop: number;
  viewportHeight: number;
  titlebarBottom: number;
  composerTop: number | null;
}

export function installHintLayout(bounds: InstallHintBounds) {
  if (!bounds.touch) return {};
  const top = Math.max(bounds.viewportTop, bounds.titlebarBottom) + 6;
  const bottom = Math.min(bounds.viewportTop + bounds.viewportHeight, bounds.composerTop ?? Infinity) - 6;
  const maxHeight = Math.max(0, bottom - top);
  return { top, bottom: "auto", maxHeight, overflowY: "auto" as const, display: maxHeight >= 44 ? undefined : "none" };
}
