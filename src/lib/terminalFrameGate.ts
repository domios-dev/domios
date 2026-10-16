export function terminalFrameVisible(active: boolean, terminalMode: boolean, frameReady: boolean): boolean {
  return active && terminalMode && frameReady;
}

export function acceptsTerminalOutput(tabId: string, ptyId: string | undefined, frameTabId?: string, framePtyId?: string): boolean {
  return tabId === frameTabId && (ptyId === undefined || ptyId === framePtyId);
}
