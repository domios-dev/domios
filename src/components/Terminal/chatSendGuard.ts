export interface LastSendStamp {
  text: string;
  at: number;
}

export const DUPLICATE_SEND_WINDOW_MS = 500;

export const DUPLICATE_SEND_NOTICE =
  "Duplicate message ignored — identical text was sent a moment ago. Wait a beat and press Enter again to resend.";

/** Double-Enter protection: identical payload inside the window is a duplicate.
 *  Callers must surface DUPLICATE_SEND_NOTICE instead of dropping silently. */
export function isDuplicateSend(last: LastSendStamp, dupKey: string, nowMs: number): boolean {
  return dupKey === last.text && nowMs - last.at < DUPLICATE_SEND_WINDOW_MS;
}
