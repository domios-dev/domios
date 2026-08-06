import { matchedOptimisticEchoes, collapseDuplicateUserMessages, type OptimisticEcho } from "./dedupe";
import { markTab, markVoice } from "./markers";
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
function userSec(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: String(Math.floor(tsMs / 1000)) };
}
// (opencode hands the frontend String(time_created) in ms).
function userMsNum(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: String(tsMs) };
}
function echo(id: string, content: string, sentAtMs: number): OptimisticEcho {
  return { id, content, sentAtMs };
}

function orderedEcho(id: string, content: string, sentAtMs: number, clientOrder: number): OptimisticEcho {
  return { id, content, sentAtMs, clientOrder };
}

const T0 = 1_700_000_000_000;

{
  const staleProviderTimestamp = T0 - 30_000;
  const parsed = user("curie-echo", "круто", staleProviderTimestamp);
  parsed.clientOrder = 12;
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [orderedEcho("opt-curie", "круто", T0, 11)],
    [parsed],
  );
  eq(matchedIds, ["opt-curie"], "newer client order reconciles a stale-timestamp echo after chat reattach");
  eq(pendingIds, [], "reattached short echo leaves no duplicate optimistic bubble");
}

{
  const text = "как то знаешь не нравятся мне они что то с ними не так везде есть свои имперфекционы";
  const parsed = user("long-echo", text.replaceAll(" ", "\n"), T0 - 45_000);
  parsed.clientOrder = 22;
  const { matchedIds } = matchedOptimisticEchoes(
    [orderedEcho("opt-long", text, T0, 21)],
    [parsed],
  );
  eq(matchedIds, ["opt-long"], "newer client order reconciles a reflowed long echo after chat reattach");
}

{
  const old = user("old", "круто", T0 - 60_000);
  old.clientOrder = 10;
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [orderedEcho("opt-new", "круто", T0, 11)],
    [old],
  );
  eq(matchedIds, [], "older client order cannot consume a newly repeated message");
  eq(pendingIds, ["opt-new"], "intentional repeat stays visible until its own echo arrives");
}

//    the provider transcript proves the message landed.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "deploy the thing", T0)],
    [],
  );
  eq(matchedIds, [], "no echo → not matched");
  eq(pendingIds, ["opt-1"], "no echo → stays pending");
}

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

//     user turn while the optimistic bubble contains only the visible body.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "check the cursor ordering bug", T0)],
    [user("a-0", markTab("Andrey", "abc12345", "check the cursor ordering bug"), T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "transport-prefixed stored echo matches optimistic body");
  eq(pendingIds, [], "transport-prefixed echo leaves no duplicate pending bubble");
}

// 3. An OLDER identical message (ts < sentAt) must NOT clear a fresh bubble.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [user("old-0", "yes", T0 - 60_000)], // a minute-old "yes" in history
  );
  eq(matchedIds, [], "stale prior identical message → not matched");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending until its own echo");
}

//    distinctly. The old loose/substring + content-dedup collapsed these.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0), echo("opt-2", "yes", T0 + 10_000)],
    [user("a-0", "yes", T0 + 300), user("b-0", "yes", T0 + 10_300)],
  );
  eq(matchedIds.sort(), ["opt-1", "opt-2"], "two sends → two distinct echoes matched");
}

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

{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "fix this /Users/me/.codeterm/uploads/x.png", T0)],
    [user("a-0", "fix this", T0 + 300)],
  );
  eq(matchedIds, ["opt-1"], "short text+attachment matches by stripped-path equality");
}

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
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "hi", T0)],
    [userSec("a-0", "hi", T0 + 500)], // seconds-stamped echo, just after send
  );
  eq(matchedIds, ["opt-1"], "seconds-stamped fresh echo → matched (skew fix)");
  eq(pendingIds, [], "seconds-stamped fresh echo leaves no stuck bubble");
}

//     message a minute older than the send still must NOT clear a fresh bubble.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [userSec("old-0", "yes", T0 - 60_000)], // a minute-old "yes", seconds units
  );
  eq(matchedIds, [], "stale seconds-stamped identical message → not matched");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending despite old seconds echo");
}

//     the NEW echo, never the stale identical line (units normalized to ms first).
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [userSec("old-0", "yes", T0 - 60_000), userSec("new-0", "yes", T0 + 400)],
  );
  eq(matchedIds, ["opt-1"], "fresh bubble matches the new seconds echo");
  eq(pendingIds, [], "fresh bubble cleared by its own (not the stale) echo");
}

//     normalization must leave genuine ms values untouched.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "deploy", T0)],
    [userMsNum("a-0", "deploy", T0 + 700)],
  );
  eq(matchedIds, ["opt-1"], "ms-number-string echo matches (no double-scaling)");
}

// 22. A local_command echo (backend-normalized form of <command-name>/new</command-name>)
//     clears the optimistic /new pill — exact content match.
{
  const localCmd = (id: string, content: string, tsMs: number): NormalizedChatMessage => ({
    id, type: "local_command", content, timestamp: new Date(tsMs).toISOString(),
  });
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-new", "/new", T0)],
    [localCmd("lc-0", "/new", T0 + 200)],
  );
  eq(matchedIds, ["opt-new"], "local_command /new echo clears optimistic pill");
  eq(pendingIds, [], "no stuck pill after local_command /new echo");
}

//     via coverage token matching.
{
  const localCmd = (id: string, content: string, tsMs: number): NormalizedChatMessage => ({
    id, type: "local_command", content, timestamp: new Date(tsMs).toISOString(),
  });
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-clear", "/clear", T0)],
    [localCmd("lc-1", "clear", T0 + 200)],
  );
  // but exact: "/clear" vs "clear" — no match, so pill stays pending (expected — content diverged)
  eq(pendingIds, ["opt-clear"], "bare-name local_command /clear stays pending (content mismatch)");
  void matchedIds; // suppress unused warning
}

// Stable correlation id reconciles an optimistic bubble with its server echo by
// identity, even when the echo's content is enriched with chips and diverges
// from the raw typed text — content heuristics cannot bridge that gap.
{
  const userTok = (id: string, content: string, tsMs: number, token: string): NormalizedChatMessage => ({
    id, type: "user", content, timestamp: new Date(tsMs).toISOString(), correlationId: token,
  });
  const echoTok = (id: string, content: string, sentAtMs: number, token: string): OptimisticEcho => ({
    id, content, sentAtMs, correlationId: token,
  });
  const token = "opt-1700000000000-42";
  // Raw typed vs chip-enriched echo share no meaningful tokens, so a match here
  // can only come from the correlation id, not exact/coverage/fuzzy content.
  const enrichedEcho = userTok("srv-9", "◆ Plan: quarterly roadmap #plan_abc ◆", T0 + 300, token);
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echoTok("opt-1700000000000-42", "look at the roadmap", T0, token)],
    [enrichedEcho],
  );
  eq(matchedIds, ["opt-1700000000000-42"], "correlation id reconciles chip-enriched echo despite content divergence");
  eq(pendingIds, [], "correlation-id match leaves no duplicate optimistic bubble");

  // Control: same divergent content WITHOUT a shared token must not match, proving
  // the reconciliation above is the token's doing, not a content heuristic.
  const untokened = matchedOptimisticEchoes(
    [echo("opt-x", "look at the roadmap", T0)],
    [user("srv-10", "◆ Plan: quarterly roadmap #plan_abc ◆", T0 + 300)],
  );
  eq(untokened.matchedIds, [], "without a shared token, chip-enriched content stays unmatched");
  eq(untokened.pendingIds, ["opt-x"], "content divergence alone keeps the bubble pending");
}

// Distinct sends carry distinct tokens: identical text with different correlation
// ids each reconcile to their OWN echo — consume-once, never collapsed into one.
{
  const userTok = (id: string, content: string, tsMs: number, token: string): NormalizedChatMessage => ({
    id, type: "user", content, timestamp: new Date(tsMs).toISOString(), correlationId: token,
  });
  const echoTok = (id: string, content: string, sentAtMs: number, token: string): OptimisticEcho => ({
    id, content, sentAtMs, correlationId: token,
  });
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echoTok("opt-a", "yes", T0, "tok-a"), echoTok("opt-b", "yes", T0 + 100, "tok-b")],
    [userTok("srv-a", "yes", T0 + 200, "tok-a"), userTok("srv-b", "yes", T0 + 300, "tok-b")],
  );
  eq(matchedIds, ["opt-a", "opt-b"], "each distinct-token repeat matches only its own echo");
  eq(pendingIds, [], "distinct-token repeats leave no bubble pending");
}

// collapseDuplicateUserMessages: a re-recorded send never paints twice, but
// genuine repeats (around a reply, or minutes apart) survive.
{
  const um = (
    id: string,
    content: string,
    tsMs: number | null,
    type: NormalizedChatMessage["type"] = "user",
  ): NormalizedChatMessage => ({
    id,
    type,
    content,
    ...(tsMs != null ? { timestamp: new Date(tsMs).toISOString() } : {}),
  });

  eq(
    collapseDuplicateUserMessages([
      um("a", "look at the plan", 1000),
      um("b", "look at the plan", 1000),
      um("c", "look at the plan", 1500),
    ]).map((m) => m.id),
    ["a"],
    "3 identical adjacent user messages collapse to the first",
  );

  eq(
    collapseDuplicateUserMessages([
      um("a", "yes", 1000),
      um("r", "ok", 1200, "assistant"),
      um("b", "yes", 1400),
    ]).map((m) => m.id),
    ["a", "r", "b"],
    "identical user messages around an assistant reply both survive",
  );

  eq(
    collapseDuplicateUserMessages([
      um("a", "continue", 1000),
      um("b", "continue", 1000 + 120_000),
    ]).map((m) => m.id),
    ["a", "b"],
    "identical user messages more than the window apart both survive",
  );

  eq(
    collapseDuplicateUserMessages([
      um("a", "run it", null),
      um("b", "run it", null),
    ]).map((m) => m.id),
    ["a"],
    "identical adjacent user messages with no timestamps collapse",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
