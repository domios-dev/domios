export function shouldWakeOnOpen(
  tab: { frozen?: boolean } | null | undefined,
  isVisible: boolean,
  alreadyRequested: boolean,
): boolean {
  return tab?.frozen === true && isVisible && !alreadyRequested;
}
