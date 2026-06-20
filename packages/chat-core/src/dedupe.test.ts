// Run: npx tsx packages/chat-core/src/dedupe.test.ts
import { matchedOptimisticEchoes, type OptimisticEcho } from "./dedupe";
import { markVoice } from "./markers";
import type { NormalizedChatMessage } from "./types";

let passed = 0;
let failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

function user(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: new Date(tsMs).toISOString() };
}
// Provider echo whose transcript timestamp is a Unix-SECONDS number (codex floors
// to seconds; RichMessage.ts is documented in seconds). The matcher must treat
// it as the same instant as a millisecond bubble send-time.
function userSec(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: String(Math.floor(tsMs / 1000)) };
}
// Provider echo whose timestamp is a millisecond epoch NUMBER as a string
// (opencode hands the frontend String(time_created) in ms).
function userMsNum(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: String(tsMs) };
}
function echo(id: string, content: string, sentAtMs: number): OptimisticEcho {
  return { id, content, sentAtMs };
}

const T0 = 1_700_000_000_000;

// 1. No real echo yet -> bubble stays pending. This helper clears only when
//    the provider transcript proves the message landed.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "deploy the thing", T0)],
    [],
  );
  eq(matchedIds, [], "no echo → not matched");
  eq(pendingIds, ["opt-1"], "no echo → stays pending");
}

// 9. Several quick sends may be accepted by a terminal TUI as one real user
//    turn. The combined transcript turn claims each covered optimistic bubble.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [
      echo("opt-1", "send many at once", T0),
      echo("opt-2", "and check it", T0 + 100),
    ],
    [user("a-0", "send many at once and check it", T0 + 500)],
  );
  eq(matchedIds, ["opt-1", "opt-2"], "combined user turn clears covered bubbles");
  eq(pendingIds, [], "combined user turn leaves no covered bubble pending");
}

// 10. Coverage is token-based and consume-once, so a one-token echo does not
//     match inside a longer token and repeated short sends need repeated tokens.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "й", T0), echo("opt-2", "йо", T0 + 100), echo("opt-3", "йо", T0 + 200)],
    [user("a-0", "йо й", T0 + 500)],
  );
  eq(matchedIds, ["opt-1", "opt-2"], "token coverage consumes exact token occurrences");
  eq(pendingIds, ["opt-3"], "missing repeated token stays pending");
}

// 11. A newer bubble is not cleared by an older combined history turn.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "fresh", T0)],
    [user("old-0", "fresh other", T0 - 60_000)],
  );
  eq(matchedIds, [], "older combined turn does not clear fresh bubble");
  eq(pendingIds, ["opt-1"], "fresh bubble remains pending");
}

// 12. CodeTerm voice markers are matched by their file identity. Provider
//     timestamps can be older than the optimistic update because transcription
//     formats the marker before the TUI records the user turn.
{
  const file = "C:\\tmp\\voice-note-123.webm";
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-voice", markVoice(file, null, "no transcriber daemon"), T0)],
    [user("voice-0", markVoice(file, "transcribed later", null), T0 - 60_000)],
  );
  eq(matchedIds, ["opt-voice"], "voice marker clears by file identity");
  eq(pendingIds, [], "voice marker leaves no duplicate pending bubble");
}

// 2. Real echo with equal content AND timestamp >= sentAt → matched (bubble cleared).
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "hello world", T0)],
    [user("uuid-a-0", "hello world", T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "real echo after send → matched");
}

// 3. An OLDER identical message (ts < sentAt) must NOT clear a fresh bubble.
//    Root cause #3: re-sending a short line ("yes") must match the NEW echo,
//    never an identical line from earlier in the history.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [user("old-0", "yes", T0 - 60_000)], // a minute-old "yes" in history
  );
  eq(matchedIds, [], "stale prior identical message → not matched");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending until its own echo");
}

// 4. Same short message sent twice → two real echoes consume-once → both cleared
//    distinctly. The old loose/substring + content-dedup collapsed these.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0), echo("opt-2", "yes", T0 + 10_000)],
    [user("a-0", "yes", T0 + 300), user("b-0", "yes", T0 + 10_300)],
  );
  eq(matchedIds.sort(), ["opt-1", "opt-2"], "two sends → two distinct echoes matched");
}

// 5. Two identical pending bubbles but only ONE real echo → consume-once: exactly
//    one cleared, the other stays pending (no double-clear off a single message).
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "continue", T0), echo("opt-2", "continue", T0 + 1_000)],
    [user("a-0", "continue", T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "single echo clears exactly one bubble (oldest first)");
  eq(pendingIds, ["opt-2"], "second identical bubble still pending");
}

// 6. Content normalization: CRLF / whitespace differences still match.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "line one\r\nline two", T0)],
    [user("a-0", "line one\nline two", T0 + 100)],
  );
  eq(matchedIds, ["opt-1"], "normalized content (CRLF) matches");
}

// 7. A parsed message with no timestamp falls back to content+consume-once match.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "no ts here", T0)],
    [{ id: "a-0", type: "user", content: "no ts here" }],
  );
  eq(matchedIds, ["opt-1"], "missing timestamp → matched on content");
}

// 8. Sub-second flooring is tolerated by the small skew (echo ts slightly < sentAt).
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "edge", T0)],
    [user("a-0", "edge", T0 - 800)], // floored within the 2s grace
  );
  eq(matchedIds, ["opt-1"], "sub-second floored echo within skew → matched");
}

// 13. A stray leaked prefix on the echoed line (e.g. "YT" glued onto the first
//     word from a focus-leaked keystroke) still clears the bubble via fuzzy
//     similarity instead of leaving a stuck dim duplicate.
{
  const real = "cannot scroll inside the input make it bigger and move the timer";
  const leaked = "YT" + real; // "YTcannot scroll ..." — first token diverges
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", leaked, T0)],
    [user("a-0", real, T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "leaked-prefix echo clears via similarity");
  eq(pendingIds, [], "leaked-prefix echo leaves no duplicate pending");
}

// 14. Optimistic bubbles carry the raw attachment path appended to the text;
//     the real echo renders the file as a chip (path stripped). They must still
//     match so the image message does not duplicate.
{
  const text = "each time I press record this happens please fix";
  const optimistic = `${text} /Users/me/.codeterm/uploads/2b9df9f1-shot.png /Users/me/.codeterm/uploads/2faa7ab5-shot.png`;
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", optimistic, T0)],
    [user("a-0", text, T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "text+attachment echo clears against chip-rendered echo");
  eq(pendingIds, [], "text+attachment echo leaves no duplicate pending");
}

// 15. Short message + attachment path: meaningful text is identical once paths
//     are stripped, so it matches even though there are too few shared tokens
//     for the looser similarity threshold.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "fix this /Users/me/.codeterm/uploads/x.png", T0)],
    [user("a-0", "fix this", T0 + 300)],
  );
  eq(matchedIds, ["opt-1"], "short text+attachment matches by stripped-path equality");
}

// 16. Fuzzy matching must NOT collapse two genuinely different short messages
//     that merely share a couple of words.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes do it", T0)],
    [user("a-0", "yes do that", T0 + 300)],
  );
  eq(matchedIds, [], "different short messages do not fuzzy-match");
  eq(pendingIds, ["opt-1"], "distinct short message stays pending");
}

// 17. A stale older message must not fuzzy-clear a fresh bubble (time guard).
{
  const text = "cannot scroll inside the input make it bigger please";
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "YT" + text, T0)],
    [user("old-0", text, T0 - 60_000)],
  );
  eq(matchedIds, [], "stale similar message does not fuzzy-clear fresh bubble");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending despite similar history");
}

// 18. Real-data skew: a fresh echo stamped in Unix SECONDS clears its bubble.
//     Before seconds→ms normalization, String(floor(T0/1000)) ≈ 1.7e9 was read
//     as epoch-ms (1970) — ~56 years before the ms send-time — so the skew guard
//     rejected EVERY real echo and the bubble stuck on "sending…" forever.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "hi", T0)],
    [userSec("a-0", "hi", T0 + 500)], // seconds-stamped echo, just after send
  );
  eq(matchedIds, ["opt-1"], "seconds-stamped fresh echo → matched (skew fix)");
  eq(pendingIds, [], "seconds-stamped fresh echo leaves no stuck bubble");
}

// 19. The seconds scaling must NOT regress the stale-echo guard: a seconds-stamped
//     message a minute older than the send still must NOT clear a fresh bubble.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [userSec("old-0", "yes", T0 - 60_000)], // a minute-old "yes", seconds units
  );
  eq(matchedIds, [], "stale seconds-stamped identical message → not matched");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending despite old seconds echo");
}

// 20. Re-sent short message with seconds stamps: consume-once + guard still pick
//     the NEW echo, never the stale identical line (units normalized to ms first).
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [userSec("old-0", "yes", T0 - 60_000), userSec("new-0", "yes", T0 + 400)],
  );
  eq(matchedIds, ["opt-1"], "fresh bubble matches the new seconds echo");
  eq(pendingIds, [], "fresh bubble cleared by its own (not the stale) echo");
}

// 21. Millisecond-number string echo (opencode shape) still matches as ms — the
//     normalization must leave genuine ms values untouched.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "deploy", T0)],
    [userMsNum("a-0", "deploy", T0 + 700)],
  );
  eq(matchedIds, ["opt-1"], "ms-number-string echo matches (no double-scaling)");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
