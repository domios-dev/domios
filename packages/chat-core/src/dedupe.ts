import { normalizeChatText } from "./normalize";
import type { NormalizedChatMessage } from "./types";

/** Grace window (ms) subtracted from an optimistic bubble's send time when
 *  matching its real echo. Absorbs sub-second timestamp flooring in some
 *  providers' session logs, while staying small enough that genuinely older
 *  history messages (seconds-to-minutes back) never satisfy the time guard —
 *  so re-sending a short message ("yes", "y") matches the NEW echo, not an
 *  identical line from earlier in the conversation. */
const ECHO_SKEW_MS = 2_000;

/** An optimistic, locally-echoed user bubble awaiting its real counterpart. */
export interface OptimisticEcho {
  id: string;
  content: string;
  /** Epoch ms the bubble was sent at (the stable client send time). */
  sentAtMs: number;
}

function timestampMs(ts: string | undefined): number | null {
  if (!ts) return null;
  // Numeric epoch-ms string (our own bubbles) or ISO-8601 (provider logs).
  const numeric = Number(ts);
  if (Number.isFinite(numeric)) return numeric;
  const parsed = Date.parse(ts);
  return Number.isNaN(parsed) ? null : parsed;
}

/** Decide which optimistic user bubbles now have a confirmed real counterpart
 *  in the parsed stream and should be dropped.
 *
 *  Consume-once: each parsed user message is claimed by AT MOST ONE bubble, so
 *  repeating a short message ("yes", "y", "continue") clears exactly one bubble
 *  per real echo — never collapsing two distinct sends into one. A bubble
 *  matches a parsed message only when their normalized content is identical AND
 *  the parsed message was written at/after the bubble's send time (minus a
 *  small skew). Bubbles with no positive match stay pending — they are NEVER
 *  evicted on a timer; only a real echo clears them.
 *
 *  `parsedUsers` must be in chronological order (oldest first) so the earliest
 *  qualifying echo is paired with the earliest pending bubble. */
export function matchedOptimisticEchoes<T extends NormalizedChatMessage>(
  optimistic: OptimisticEcho[],
  parsedUsers: T[],
): { matchedIds: string[]; pendingIds: string[] } {
  const consumed = new Array<boolean>(parsedUsers.length).fill(false);
  const matchedIds: string[] = [];
  const pendingIds: string[] = [];
  for (const echo of optimistic) {
    const wanted = normalizeChatText(echo.content).toLowerCase();
    let hit = -1;
    if (wanted) {
      for (let i = 0; i < parsedUsers.length; i++) {
        if (consumed[i]) continue;
        const candidate = parsedUsers[i];
        if (candidate.type !== "user") continue;
        if (normalizeChatText(candidate.content).toLowerCase() !== wanted) continue;
        const ts = timestampMs(candidate.timestamp);
        if (ts != null && ts < echo.sentAtMs - ECHO_SKEW_MS) continue;
        hit = i;
        break;
      }
    }
    if (hit >= 0) {
      consumed[hit] = true;
      matchedIds.push(echo.id);
    } else {
      pendingIds.push(echo.id);
    }
  }
  return { matchedIds, pendingIds };
}
