import { normalizeChatText } from "./normalize";
import { parseMarker } from "./markers";
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

interface CoverageToken {
  value: string;
  used: boolean;
}

function fileName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

function markerEchoMatchesParsed(echoContent: string, parsedContent: string): boolean {
  const echo = parseMarker(echoContent);
  const parsed = parseMarker(parsedContent);
  if (!echo || !parsed || echo.kind !== parsed.kind) return false;
  const echoFile = echo.attrs.file;
  const parsedFile = parsed.attrs.file;
  if (!echoFile || !parsedFile) return false;
  return echoFile === parsedFile || fileName(echoFile) === fileName(parsedFile);
}

function coverageTokens(content: string): CoverageToken[] {
  return normalizeChatText(content)
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .map((value) => ({ value, used: false }));
}

function tokenNeedle(content: string): string[] {
  return normalizeChatText(content).toLowerCase().split(/\s+/).filter(Boolean);
}

function consumeTokenSequence(tokens: CoverageToken[], needle: string[]): boolean {
  if (needle.length === 0 || tokens.length < needle.length) return false;
  for (let start = 0; start <= tokens.length - needle.length; start++) {
    let ok = true;
    for (let offset = 0; offset < needle.length; offset++) {
      const token = tokens[start + offset];
      if (token.used || token.value !== needle[offset]) {
        ok = false;
        break;
      }
    }
    if (!ok) continue;
    for (let offset = 0; offset < needle.length; offset++) {
      tokens[start + offset].used = true;
    }
    return true;
  }
  return false;
}

/** Decide which optimistic user bubbles now have a confirmed real counterpart
 *  in the parsed stream and should be dropped.
 *
 *  Consume-once: each parsed user message is claimed by AT MOST ONE bubble, so
 *  repeating a short message ("yes", "y", "continue") clears exactly one bubble
 *  per real echo — never collapsing two distinct sends into one. A bubble
 *  First pass is exact content matching. Second pass handles terminal TUIs that
 *  accepted several quick chat sends as one real user turn: each pending bubble
 *  can be claimed by one matching token sequence inside that combined parsed
 *  turn. Bubbles with no positive match stay pending; callers may decide how to
 *  surface or expire those, but this helper only clears with transcript proof.
 *
 *  `parsedUsers` must be in chronological order (oldest first) so the earliest
 *  qualifying echo is paired with the earliest pending bubble. */
export function matchedOptimisticEchoes<T extends NormalizedChatMessage>(
  optimistic: OptimisticEcho[],
  parsedUsers: T[],
): { matchedIds: string[]; pendingIds: string[] } {
  const consumed = new Array<boolean>(parsedUsers.length).fill(false);
  const exactMatched = new Set<string>();
  const coveredMatched = new Set<string>();
  const pendingIds: string[] = [];
  for (const echo of optimistic) {
    const wanted = normalizeChatText(echo.content).toLowerCase();
    let hit = -1;
    if (wanted) {
      for (let i = 0; i < parsedUsers.length; i++) {
        if (consumed[i]) continue;
        const candidate = parsedUsers[i];
        if (candidate.type !== "user") continue;
        const markerMatch = markerEchoMatchesParsed(echo.content, candidate.content);
        if (!markerMatch && normalizeChatText(candidate.content).toLowerCase() !== wanted) continue;
        const ts = timestampMs(candidate.timestamp);
        if (!markerMatch && ts != null && ts < echo.sentAtMs - ECHO_SKEW_MS) continue;
        hit = i;
        break;
      }
    }
    if (hit >= 0) {
      consumed[hit] = true;
      exactMatched.add(echo.id);
    }
  }
  const coverage = parsedUsers.map((message, index) => ({
    tokens: consumed[index] ? [] : coverageTokens(message.content),
    timestamp: timestampMs(message.timestamp),
  }));
  for (const echo of optimistic) {
    if (exactMatched.has(echo.id)) continue;
    const wanted = tokenNeedle(echo.content);
    if (wanted.length === 0) continue;
    for (const candidate of coverage) {
      if (candidate.timestamp != null && candidate.timestamp < echo.sentAtMs - ECHO_SKEW_MS) {
        continue;
      }
      if (consumeTokenSequence(candidate.tokens, wanted)) {
        coveredMatched.add(echo.id);
        break;
      }
    }
  }
  const matchedIds: string[] = [];
  for (const echo of optimistic) {
    if (exactMatched.has(echo.id) || coveredMatched.has(echo.id)) {
      matchedIds.push(echo.id);
    } else {
      pendingIds.push(echo.id);
    }
  }
  return { matchedIds, pendingIds };
}
