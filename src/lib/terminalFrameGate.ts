export function terminalFrameVisible(active: boolean, terminalMode: boolean, frameReady: boolean): boolean {
  return active && terminalMode && frameReady;
}

export function terminalLayoutRequired(active: boolean, terminalMode: boolean, termReady: boolean): boolean {
  return active && terminalMode && termReady;
}

export interface TerminalContentLayout {
  terminalMode: boolean;
  touchUi: boolean;
  fallbackChat: boolean;
  keyBar: boolean;
}

/**
 * The tab top bar paints a fade over the content below it; plain terminal mode
 * reserves that band so PTY row 0 is never painted under it.
 */
export function terminalContentTopInset(layout: TerminalContentLayout): string | number {
  if (layout.touchUi && layout.terminalMode) return "calc(1.25rem + var(--ct-safe-top, 0px))";
  if (layout.fallbackChat || layout.keyBar) return 8;
  if (layout.terminalMode) return "var(--ct-top-bar-fade, 12px)";
  return 0;
}

export function acceptsTerminalOutput(tabId: string, ptyId: string | undefined, frameTabId?: string, framePtyId?: string): boolean {
  return tabId === frameTabId && (ptyId === undefined || ptyId === framePtyId);
}
