import { normalizeChatText } from "./normalize";
import { parseMarker } from "./markers";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";
import { userVisibleText } from "./userAttachments";

/** Grace window (ms) subtracted from an optimistic bubble's send time when
 *  matching its real echo. Absorbs sub-second timestamp flooring in some
 *  providers' session logs, while staying small enough that genuinely older
 *  history messages (seconds-to-minutes back) never satisfy the time guard —
 *  so re-sending a short message ("yes", "y") matches the NEW echo, not an
 *  identical line from earlier in the conversation. */
export const ECHO_SKEW_MS = 2_000;

/** Fuzzy fallback: clears an echo whose real counterpart diverged slightly —
 *  a leaked stray prefix, or the raw attachment path that the optimistic bubble
 *  carries but the real echo renders as a chip. Thresholds are deliberately
 *  high so genuinely different messages never collapse. */
const FUZZY_DICE_THRESHOLD = 0.6;
const FUZZY_MIN_SHARED_TOKENS = 3;

/** An optimistic, locally-echoed user bubble awaiting its real counterpart. */
export interface OptimisticEcho {
  id: string;
  content: string;
  /** Epoch ms the bubble was sent at (the stable client send time). */
  sentAtMs: number;
  /** Stable client send-correlation token. When the server echoes it onto the
   *  committed turn, reconciliation is by identity instead of content. */
  correlationId?: string;
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

function matchableContent(content: string): string {
  return parseMarker(content)?.payload ?? content;
}

function isFileIdentityMarker(content: string): boolean {
  const marker = parseMarker(content);
  return marker?.kind === "voice" || marker?.kind === "photo" || marker?.kind === "file";
}

function coverageTokens(content: string): CoverageToken[] {
  return normalizeChatText(matchableContent(content))
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .map((value) => ({ value, used: false }));
}

function tokenNeedle(content: string): string[] {
  return normalizeChatText(matchableContent(content)).toLowerCase().split(/\s+/).filter(Boolean);
}

function unquoted(token: string): string {
  return token.replace(/^['"`]+|['"`]+$/g, "");
}

function isPathLikeToken(token: string): boolean {
  const bare = unquoted(token);
  return /^(\/|~\/|\.\.?\/|[a-z]:[\\/])/i.test(bare);
}

// bubble's appended raw path never blocks a match against its chip-rendered echo.
function meaningfulTokens(content: string): string[] {
  return normalizeChatText(matchableContent(content))
    .toLowerCase()
    .split(/\s+/)
    .filter((token) => token.length > 0 && !isPathLikeToken(token));
}

// The bubble carries the raw path; the transcript row carries it as an attachment.
function attachmentEchoMatches(echoContent: string, candidate: NormalizedChatMessage): boolean {
  const names = (candidate.attachments ?? [])
    .map((attachment) => (attachment.path ? fileName(attachment.path).toLowerCase() : ""))
    .filter(Boolean);
  if (names.length === 0) return false;
  const tokens = normalizeChatText(matchableContent(echoContent)).split(/\s+/).filter(Boolean);
  const echoNames = tokens.filter(isPathLikeToken).map((token) => fileName(unquoted(token)).toLowerCase());
  if (echoNames.length !== names.length || !echoNames.every((name) => names.includes(name))) return false;
  const echoText = tokens.filter((token) => !isPathLikeToken(token)).join(" ").toLowerCase();
  return echoText === normalizeChatText(userVisibleText(candidate.content, candidate.attachments)).toLowerCase();
}

function diceSimilarity(a: string[], b: string[]): { dice: number; shared: number } {
  if (a.length === 0 || b.length === 0) return { dice: 0, shared: 0 };
  const counts = new Map<string, number>();
  for (const token of a) counts.set(token, (counts.get(token) ?? 0) + 1);
  let shared = 0;
  for (const token of b) {
    const remaining = counts.get(token) ?? 0;
    if (remaining > 0) {
      shared++;
      counts.set(token, remaining - 1);
    }
  }
  return { dice: (2 * shared) / (a.length + b.length), shared };
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

function candidateFollowsEcho<T extends NormalizedChatMessage>(echo: OptimisticEcho, candidate: T): boolean {
  const ts = timestampMs(candidate.timestamp);
  return ts == null || ts >= echo.sentAtMs - ECHO_SKEW_MS;
}

/** Exact echoes are single-use, while one row may confirm an ordered send sequence. */
export function matchedOptimisticEchoes<T extends NormalizedChatMessage>(
  optimistic: OptimisticEcho[],
  parsedUsers: T[],
): { matchedIds: string[]; pendingIds: string[]; claims: Array<{ echoId: string; parsedIndex: number }> } {
  const claims: Array<{ echoId: string; parsedIndex: number }> = [];
  const consumed = new Array<boolean>(parsedUsers.length).fill(false);
  const idMatched = new Set<string>();
  const exactMatched = new Set<string>();
  const coveredMatched = new Set<string>();
  const claimedRowsById = new Map<string, number>();
  const pendingIds: string[] = [];
  // Consume-once, and no ordering guard — a token is globally unique to its send, so its echo is unambiguous.
  for (const echo of optimistic) {
    if (!echo.correlationId) continue;
    for (let i = 0; i < parsedUsers.length; i++) {
      if (consumed[i]) continue;
      const candidate = parsedUsers[i];
      if (candidate.type !== "user" && candidate.type !== "local_command") continue;
      if (candidate.correlationId == null || candidate.correlationId !== echo.correlationId) continue;
      consumed[i] = true;
      idMatched.add(echo.id);
      claimedRowsById.set(echo.id, i);
      claims.push({ echoId: echo.id, parsedIndex: i });
      break;
    }
  }
  for (const echo of optimistic) {
    if (idMatched.has(echo.id)) continue;
    const wanted = normalizeChatText(matchableContent(echo.content)).toLowerCase();
    let hit = -1;
    if (wanted) {
      for (let i = 0; i < parsedUsers.length; i++) {
        if (consumed[i]) continue;
        const candidate = parsedUsers[i];
        if (candidate.type !== "user" && candidate.type !== "local_command") continue;
        const markerMatch = markerEchoMatchesParsed(echo.content, candidate.content);
        if (
          !markerMatch
          && normalizeChatText(matchableContent(candidate.content)).toLowerCase() !== wanted
          && !attachmentEchoMatches(echo.content, candidate)
        ) {
          continue;
        }
        if (!markerMatch && !candidateFollowsEcho(echo, candidate)) continue;
        hit = i;
        break;
      }
    }
    if (hit >= 0) {
      consumed[hit] = true;
      exactMatched.add(echo.id);
      claimedRowsById.set(echo.id, hit);
      claims.push({ echoId: echo.id, parsedIndex: hit });
    }
  }
  const textSequenceMatched = new Set<string>();
  for (let rowIndex = 0; rowIndex < parsedUsers.length; rowIndex += 1) {
    const candidate = parsedUsers[rowIndex];
    if ((candidate.type !== "user" && candidate.type !== "local_command")) continue;
    const text = normalizeChatText(matchableContent(candidate.content)).toLowerCase();
    if (!text) continue;
    for (let start = 0; start < optimistic.length; start += 1) {
      const first = optimistic[start];
      if (!candidateFollowsEcho(first, candidate)) continue;
      const firstOwner = claimedRowsById.get(first.id);
      if (firstOwner != null && firstOwner !== rowIndex) continue;
      const firstText = normalizeChatText(matchableContent(first.content)).toLowerCase();
      if (!firstText || !text.startsWith(firstText)) continue;
      let offset = firstText.length;
      const matched = [first];
      for (let next = start + 1; next < optimistic.length; next += 1) {
        const send = optimistic[next];
        if (!candidateFollowsEcho(send, candidate)) break;
        const owner = claimedRowsById.get(send.id);
        if (owner != null && owner !== rowIndex) break;
        while (offset < text.length && /\s/.test(text[offset] ?? "")) offset += 1;
        const sendText = normalizeChatText(matchableContent(send.content)).toLowerCase();
        if (!sendText || !text.startsWith(sendText, offset)) break;
        offset += sendText.length;
        matched.push(send);
        if (matched.length >= 2 && text.slice(offset).trim() === "") {
          break;
        }
      }
      if (matched.length < 2 || text.slice(offset).trim() !== "") continue;
      consumed[rowIndex] = true;
      for (const item of matched) {
        if (claimedRowsById.has(item.id)) continue;
        textSequenceMatched.add(item.id);
        claimedRowsById.set(item.id, rowIndex);
        claims.push({ echoId: item.id, parsedIndex: rowIndex });
      }
    }
  }
  const coverageClaimed = new Set<number>();
  const coverage = parsedUsers.map((message, index) => ({
    index,
    message,
    tokens: consumed[index] ? [] : coverageTokens(message.content),
  }));
  for (const echo of optimistic) {
    if (idMatched.has(echo.id) || exactMatched.has(echo.id) || textSequenceMatched.has(echo.id)) continue;
    const wanted = tokenNeedle(echo.content);
    if (wanted.length === 0) continue;
    for (const candidate of coverage) {
      if (!candidateFollowsEcho(echo, candidate.message)) continue;
      if (consumeTokenSequence(candidate.tokens, wanted)) {
        coveredMatched.add(echo.id);
        coverageClaimed.add(candidate.index);
        claims.push({ echoId: echo.id, parsedIndex: candidate.index });
        break;
      }
    }
  }
  const fuzzyMatched = new Set<string>();
  for (const echo of optimistic) {
    if (idMatched.has(echo.id) || exactMatched.has(echo.id) || coveredMatched.has(echo.id)) continue;
    if (isFileIdentityMarker(echo.content)) continue;
    const echoTokens = meaningfulTokens(echo.content);
    if (echoTokens.length === 0) continue;
    const echoText = echoTokens.join(" ");
    let hit = -1;
    let bestDice = FUZZY_DICE_THRESHOLD;
    for (let i = 0; i < parsedUsers.length; i++) {
      if (consumed[i] || coverageClaimed.has(i)) continue;
      const candidate = parsedUsers[i];
      if ((candidate.type !== "user" && candidate.type !== "local_command") || isFileIdentityMarker(candidate.content)) continue;
      if (!candidateFollowsEcho(echo, candidate)) continue;
      const candidateTokens = meaningfulTokens(candidate.content);
      if (candidateTokens.length === 0) continue;
      if (candidateTokens.join(" ") === echoText) {
        hit = i;
        break;
      }
      const { dice, shared } = diceSimilarity(echoTokens, candidateTokens);
      if (shared >= FUZZY_MIN_SHARED_TOKENS && dice > bestDice) {
        bestDice = dice;
        hit = i;
      }
    }
    if (hit >= 0) {
      consumed[hit] = true;
      fuzzyMatched.add(echo.id);
      claims.push({ echoId: echo.id, parsedIndex: hit });
    }
  }
  const matchedIds: string[] = [];
  for (const echo of optimistic) {
    if (
      idMatched.has(echo.id)
      || exactMatched.has(echo.id)
      || textSequenceMatched.has(echo.id)
      || coveredMatched.has(echo.id)
      || fuzzyMatched.has(echo.id)
    ) {
      matchedIds.push(echo.id);
    } else {
      pendingIds.push(echo.id);
    }
  }
  return { matchedIds, pendingIds, claims };
}

/** Window (ms) within which two consecutive user messages with identical content
 *  are the same logical send recorded twice (a re-read transcript tail, a double
 *  delivery) rather than a deliberate resend. Generous enough to collapse an
 *  impatient double-send, while a genuine repeat minutes later — in practice
 *  always with an assistant turn between it — survives. */
const USER_DUP_WINDOW_MS = 60_000;

/** Drop consecutive user messages that repeat the immediately-preceding user
 *  message's content within {@link USER_DUP_WINDOW_MS}. View-layer only: the
 *  stored transcript keeps every record; this just stops the chat UI painting
 *  the same bubble two or three times. Messages carrying attachments never
 *  collapse (their content may match while the attachment differs). */
export function collapseDuplicateUserMessages<T extends NormalizedChatMessage>(messages: T[]): T[] {
  const out: T[] = [];
  for (const message of messages) {
    const prev = out[out.length - 1];
    const content = message.type === "user" ? normalizeChatText(message.content).toLowerCase() : "";
    if (
      prev
      && prev.type === "user"
      && message.type === "user"
      && content.length > 0
      && !message.attachments?.length
      && !(prev.seq != null && message.seq != null && prev.seq !== message.seq)
      && content === normalizeChatText(prev.content).toLowerCase()
    ) {
      const pt = timestampMs(prev.timestamp);
      const mt = timestampMs(message.timestamp);
      if (pt == null || mt == null || Math.abs(mt - pt) <= USER_DUP_WINDOW_MS) {
        continue;
      }
    }
    out.push(message);
  }
  return out;
}
