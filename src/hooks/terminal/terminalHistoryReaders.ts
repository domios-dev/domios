/** Tabs whose hidden terminal was left scrolled into history. Unmounting one would
 *  replay it at the tail, so the mount budget keeps them warm instead. */
const readers = new Set<string>();

export function setTerminalReadingHistory(tabId: string, reading: boolean): void {
  if (reading) readers.add(tabId);
  else readers.delete(tabId);
}

export function isTerminalReadingHistory(tabId: string): boolean {
  return readers.has(tabId);
}
