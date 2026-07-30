export interface LastSendStamp {
  text: string;
  at: number;
}

export const DUPLICATE_SEND_WINDOW_MS = 500;

/** Double-Enter protection: identical payload inside the window is a duplicate.
 *  Callers log the suppression without surfacing delivery state in Chat. */
export function isDuplicateSend(last: LastSendStamp, dupKey: string, nowMs: number): boolean {
  return dupKey === last.text && nowMs - last.at < DUPLICATE_SEND_WINDOW_MS;
}
