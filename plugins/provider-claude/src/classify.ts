// AS3/AS4 pane-state classification (rate-limit / clarifying / auth) ──────────
//
// Wire shapes mirror src-tauri/src/agent/pane_state.rs (camelCase JS boundary,
// snake_case PaneState strings). Declared locally because the published
// @codeterm/plugin-sdk PluginModule exposes classifyPaneState only via its index
// signature, not a named type.
import { usageParseTodayClock } from "./usage";

export interface PaneStateSnapshot {
  paneId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  screenText: string;
}

type PaneState =
  | "working"
  | "waiting_on_auth"
  | "update_prompt"
  | "rate_limited"
  | "clarifying_question"
  | "idle_stuck"
  | "unknown";

interface PaneStateAction {
  keys?: string[];
  delayedResume?: { at: string; text: string; thenKeys?: string[] };
  surface?: { kind: string; payload: unknown };
}

export interface PaneStateClassification {
  state: PaneState;
  confidence: number;
  data?: { resetAt?: string; questions?: string[] };
  action?: PaneStateAction;
}

// Match the core reactor's GLOBAL_THRESHOLD (state_reactor.rs). The plugin only
// emits an actionable verdict at/above this confidence, so it never recommends
// a key-press the reactor would itself reject.
const CLASSIFY_THRESHOLD = 0.8;

// Fire the resume a minute past the parsed reset, absorbing clock skew + the
// provider's own grace so `continue your work` lands after the limit clears.
const RESUME_BUFFER_MS = 60 * 1000;

// AS3 (Case B: rate-limit menu) + AS4 (clarifying question / auth) — classify a
// parked claude pane from its rendered screen. The core reactor
// (state_reactor.rs) only calls this on the idle/waiting edge and only
// auto-acts when confidence >= GLOBAL_THRESHOLD (0.8), the state is NOT
// waiting_on_auth, and an action is present. All claude-specific knowledge
// (the rate-limit fingerprint, the reset-time/timezone parse, the confirm-wait
// navigation, the auth guard) lives here; core stays generic.
//
// Guarantees this method enforces:
//  1. Auth is sacred — a login/OAuth/API-key screen is waiting_on_auth with NO
//     keys, surfaced and never auto-pressed.
//  2. No false positives — every actionable verdict is an ENSEMBLE of
//     independent STRUCTURAL signals (a numbered menu shape, a reset-clock
//     pattern, a confirm/cancel footer — never literal vendor copy). A single
//     weak match stays below threshold. An actively-streaming screen (busy
//     markers) is vetoed to `working` even if a menu is visible.
//  3. Rate-limit → confirm-wait by NAVIGATING to the wait option (never a blind
//     Enter on whatever is highlighted) + a delayed `continue your work` resume
//     scheduled at the parsed reset time + buffer. An unparseable reset time
//     surfaces to the user instead of auto-scheduling at a wrong moment.
//  4. Clarifying question → surface the extracted question(s); NEVER auto-answer
//     (no keys), so the user decides.
export function classifyClaudePaneState(snapshot: PaneStateSnapshot): PaneStateClassification | null {
  // Normalize CRLF/CR → LF: Windows pane output is CRLF, and every structural
  // pattern below matches on \n boundaries — a trailing \r breaks the matches.
  const screen = String((snapshot && snapshot.screenText) || "").replace(/\r\n?/g, "\n");
  if (!screen.trim()) return null;

  // 1. Auth first — sacred, and takes precedence over any menu beneath it.
  const authConfidence = scoreAuthPrompt(screen);
  if (authConfidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "waiting_on_auth",
      confidence: authConfidence,
      action: { surface: { kind: "auth_prompt", payload: { reason: "login_detected" } } },
    };
  }

  // Busy veto: a streaming pane is never treated as parked, so a menu rendered
  // mid-turn can't be mistaken for a prompt awaiting input.
  if (looksBusy(screen)) return { state: "working", confidence: 0 };

  // 2. Rate-limit menu → confirm-wait + schedule resume.
  const rl = scoreRateLimit(screen, host.unixNowMs());
  if (rl.confidence >= CLASSIFY_THRESHOLD && rl.waitKeys && rl.waitKeys.length > 0) {
    const action: PaneStateAction = { keys: rl.waitKeys };
    const data: { resetAt?: string } = {};
    if (rl.resetAtMs !== null) {
      data.resetAt = new Date(rl.resetAtMs).toISOString();
      action.delayedResume = {
        at: new Date(rl.resetAtMs + RESUME_BUFFER_MS).toISOString(),
        text: "continue your work",
        thenKeys: ["enter"],
      };
    } else {
      // High-confidence menu but the reset time didn't parse — confirm the wait
      // (safe), but surface for a manual resume rather than schedule blindly.
      action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
    }
    return {
      state: "rate_limited",
      confidence: rl.confidence,
      data: rl.resetAtMs !== null ? data : undefined,
      action: action,
    };
  }

  // 3. Clarifying question → surface only, never auto-answer.
  const cq = scoreClarifyingQuestion(screen);
  if (cq.confidence >= CLASSIFY_THRESHOLD && cq.questions.length > 0) {
    return {
      state: "clarifying_question",
      confidence: cq.confidence,
      data: { questions: cq.questions },
      action: { surface: { kind: "clarifying_question", payload: { questions: cq.questions } } },
    };
  }

  // Some signal, but not a confident actionable verdict → no action, so the
  // reactor never fires on a partial read.
  const partial = rl.confidence > cq.confidence ? rl.confidence : cq.confidence;
  if (partial > 0) return { state: "unknown", confidence: partial };

  return { state: "working", confidence: 0 };
}

interface ClaudeMenuOption { highlighted: boolean; label: string }

// A line of the form `> 1. Label` / `❯ 2. Label` / `  3. Label`. The leading
// glyph (when present) marks the currently-highlighted option.
const CLAUDE_MENU_OPTION_RE = /^[ \t]*([>❯▶➤])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;

function parseClaudeMenuOptions(screen: string): ClaudeMenuOption[] {
  const lines = String(screen || "").split("\n");
  const options: ClaudeMenuOption[] = [];
  for (let i = 0; i < lines.length; i++) {
    const m = CLAUDE_MENU_OPTION_RE.exec(lines[i]);
    if (!m) continue;
    options.push({ highlighted: !!m[1], label: m[3] });
  }
  return options;
}

// Keys to move from the highlighted option to `targetIdx`, then confirm. Never
// emits a bare Enter on a foreign highlight — the caller must have located a
// concrete target option first.
function navigationKeys(options: ClaudeMenuOption[], targetIdx: number): string[] {
  let highlightIdx = -1;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) { highlightIdx = i; break; }
  }
  if (highlightIdx === -1) highlightIdx = 0; // claude defaults to option 1
  const keys: string[] = [];
  const delta = targetIdx - highlightIdx;
  if (delta > 0) for (let i = 0; i < delta; i++) keys.push("down");
  else if (delta < 0) for (let i = 0; i < -delta; i++) keys.push("up");
  keys.push("enter");
  return keys;
}

// Active-stream markers: the agent is working, not parked. Structural footer /
// status fragments common to a mid-turn screen — a hard veto so a menu rendered
// while the model streams is never read as a prompt awaiting input.
function looksBusy(screen: string): boolean {
  const s = String(screen || "");
  return (
    /\besc to interrupt\b/i.test(s) ||
    /\bctrl\+c to (?:cancel|stop|interrupt|quit)\b/i.test(s) ||
    /\bWorking[….]/.test(s) ||
    /\((?:esc|ctrl\+c) to interrupt\)/i.test(s)
  );
}

interface RateLimitScore { confidence: number; waitKeys: string[] | null; resetAtMs: number | null }

// Independent STRUCTURAL signals for a usage/rate-limit menu; weights sum to 1.0.
// The actionable core is a numbered menu offering BOTH a wait/reset option and an
// upgrade option — the corroborating limit phrase, reset-clock, and confirm
// footer push a real prompt over threshold while any lone signal stays well
// under. Matched on shape (a menu carrying "wait" vs "upgrade" choices, a clock
// with am/pm), never on exact vendor wording.
function scoreRateLimit(screen: string, nowMs: number): RateLimitScore {
  const options = parseClaudeMenuOptions(screen);
  let waitIdx = -1;
  let hasUpgradeOption = false;
  for (let i = 0; i < options.length; i++) {
    const label = options[i].label;
    if (/\bupgrade\b/i.test(label)) hasUpgradeOption = true;
    if (waitIdx === -1 && /\b(?:wait|stop and wait)\b/i.test(label) && !/\bupgrade\b/i.test(label)) {
      waitIdx = i;
    }
  }
  const sigWaitUpgradeMenu = waitIdx !== -1 && hasUpgradeOption;       // 0.50
  const sigLimitWord =
    /\b(?:session|usage|rate|message|weekly)\s+limit\b/i.test(screen) ||
    /\blimit\b[\s\S]{0,40}\breset/i.test(screen);                       // 0.25
  const sigResetClock = /\breset[s]?\b[\s\S]{0,40}\d{1,2}(?::\d{2})?\s*(?:am|pm)\b/i.test(screen); // 0.15
  const sigConfirmFooter = /\bto confirm\b/i.test(screen) && /\bto (?:cancel|exit)\b/i.test(screen); // 0.10

  let confidence = 0;
  if (sigWaitUpgradeMenu) confidence += 0.5;
  if (sigLimitWord) confidence += 0.25;
  if (sigResetClock) confidence += 0.15;
  if (sigConfirmFooter) confidence += 0.1;
  if (confidence > 1) confidence = 1;

  // Only navigate when the actionable menu signal is present.
  const waitKeys = sigWaitUpgradeMenu ? navigationKeys(options, waitIdx) : null;
  const resetAtMs = parseResetClock(screen, nowMs);
  return { confidence: confidence, waitKeys: waitKeys, resetAtMs: resetAtMs };
}

// Parse the reset clock + (optional) timezone, e.g. `resets 4:20pm (Europe/Riga)`.
// Claude prints the reset in the machine's LOCAL timezone (the parenthesized zone
// IS the user's zone, since the daemon shares the user's clock), so we resolve
// the next occurrence in local time — the same assumption the /usage parser makes
// in usageParseTodayClock. Returns null when no clock is present (the caller then
// surfaces for a manual resume instead of scheduling at a guessed instant).
function parseResetClock(screen: string, nowMs: number): number | null {
  const m = /\breset[s]?\b[^\n]{0,40}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(String(screen || ""));
  if (!m) return null;
  return usageParseTodayClock(m[1], nowMs);
}

interface ClarifyScore { confidence: number; questions: string[] }

// Independent STRUCTURAL signals for a parked clarifying-question prompt; weights
// sum to 1.0. A real "the agent is asking you to choose" screen carries a
// numbered menu, a question line, and a confirm/submit footer together; a lone
// question in mid-stream prose stays under threshold (and a busy screen is vetoed
// upstream). Never auto-answered — the verdict only surfaces the question(s).
function scoreClarifyingQuestion(screen: string): ClarifyScore {
  const options = parseClaudeMenuOptions(screen);
  const questions = extractQuestions(screen);
  const sigMenu = options.length >= 2;                                  // 0.35
  const sigQuestion = questions.length > 0;                             // 0.30
  const sigConfirmFooter =
    /\bto (?:confirm|submit|select)\b/i.test(screen);                   // 0.35

  let confidence = 0;
  if (sigMenu) confidence += 0.35;
  if (sigQuestion) confidence += 0.3;
  if (sigConfirmFooter) confidence += 0.35;
  if (confidence > 1) confidence = 1;
  return { confidence: confidence, questions: questions };
}

// Lines that end in a question mark, trimmed. The surfaced payload for a
// clarifying prompt — what the user is being asked, verbatim.
function extractQuestions(screen: string): string[] {
  const lines = String(screen || "").split("\n");
  const out: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line.length > 1 && line.charAt(line.length - 1) === "?") out.push(line);
  }
  return out;
}

// Auth ensemble. Weighted so no single casual mention fires, but a real
// login/OAuth/API-key screen clears threshold. Auth false positives are the SAFE
// direction (surface, never auto-act) — so this leans inclusive.
function scoreAuthPrompt(screen: string): number {
  const s = String(screen || "");
  let score = 0;
  if (/\bsign[ -]?in\b/i.test(s)) score += 0.5;
  if (/\blog[ -]?in\b/i.test(s)) score += 0.5;
  if (/\bauthenticat(?:e|ion|ing)\b/i.test(s)) score += 0.5;
  if (/\bo\s?auth\b/i.test(s)) score += 0.6;
  if (/\bapi key\b/i.test(s)) score += 0.6;
  if (/paste\b[^\n]*\b(token|key|code|credential)/i.test(s)) score += 0.6;
  if (/\bauthorization code\b/i.test(s)) score += 0.6;
  if (/enter your\b[^\n]*\b(api key|token|credential)/i.test(s)) score += 0.6;
  return score > 1 ? 1 : score;
}
