import { usageParseTodayClock } from "./usage";

export interface TabStateSnapshot {
  tabId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  screenText: string;
}

type TabState =
  | "working"
  | "waiting_on_auth"
  | "update_prompt"
  | "rate_limited"
  | "clarifying_question"
  | "idle_stuck"
  | "unknown";

interface TabStateAction {
  keys?: string[];
  delayedResume?: { at: string; text: string; thenKeys?: string[] };
  surface?: { kind: string; payload: unknown };
}

export interface TabStateClassification {
  state: TabState;
  confidence: number;
  data?: { resetAt?: string; questions?: string[] };
  action?: TabStateAction;
}

// Must match the core reactor's GLOBAL_THRESHOLD (state_reactor.rs) — below it the reactor would reject the verdict.
const CLASSIFY_THRESHOLD = 0.8;

// Buffer past the parsed reset so `continue your work` lands after the limit actually clears.
const RESUME_BUFFER_MS = 60 * 1000;

export function classifyClaudeTabState(snapshot: TabStateSnapshot): TabStateClassification | null {
  // Normalize CRLF/CR → LF: the structural patterns below match on \n boundaries.
  const screen = String((snapshot && snapshot.screenText) || "").replace(/\r\n?/g, "\n");
  if (!screen.trim()) return null;

  // Busy veto: a streaming tab is never parked, so a mid-turn menu isn't read as a prompt.
  if (looksBusy(screen)) return { state: "working", confidence: 0 };

  const activeChoice = activeTerminalChoiceBlock(screen);
  const authConfidence = activeChoice
    ? scoreAuthPrompt(activeChoice.options.map((option) => option.label).join("\n"))
    : 0;
  if (authConfidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "waiting_on_auth",
      confidence: authConfidence,
      action: { surface: { kind: "auth_prompt", payload: { reason: "login_detected" } } },
    };
  }

  // Rate-limit menu → confirm-wait + schedule resume.
  const rl = scoreRateLimit(screen, host.unixNowMs());
  if (rl.confidence >= CLASSIFY_THRESHOLD && rl.waitKeys && rl.waitKeys.length > 0) {
    const action: TabStateAction = { keys: rl.waitKeys };
    const data: { resetAt?: string } = {};
    if (rl.resetAtMs !== null) {
      data.resetAt = new Date(rl.resetAtMs).toISOString();
      action.delayedResume = {
        at: new Date(rl.resetAtMs + RESUME_BUFFER_MS).toISOString(),
        text: "continue your work",
        thenKeys: ["enter"],
      };
    } else {
      // Reset time didn't parse — confirm the wait but surface for a manual resume rather than schedule blindly.
      action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
    }
    return {
      state: "rate_limited",
      confidence: rl.confidence,
      data: rl.resetAtMs !== null ? data : undefined,
      action: action,
    };
  }

  // Clarifying question → surface only, never auto-answer.
  const cq = scoreClarifyingQuestion(screen);
  if (cq.confidence >= CLASSIFY_THRESHOLD && cq.questions.length > 0) {
    return {
      state: "clarifying_question",
      confidence: cq.confidence,
      data: { questions: cq.questions },
      action: { surface: { kind: "clarifying_question", payload: { questions: cq.questions } } },
    };
  }

  // Some signal but not a confident verdict → no action, so the reactor never fires on a partial read.
  const partial = rl.confidence > cq.confidence ? rl.confidence : cq.confidence;
  if (partial > 0) return { state: "unknown", confidence: partial };

  return { state: "working", confidence: 0 };
}

interface ClaudeMenuOption { highlighted: boolean; label: string }

// A menu line like `> 1. Label`; the leading glyph (if present) marks the highlighted option.
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

// Keys to move from the highlighted option to `targetIdx`, then confirm — the caller must have located a concrete target.
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

// Active-stream markers: a hard veto so a menu rendered mid-stream is never read as a prompt.
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

// numbered menu offering both wait and upgrade; matched on shape, never vendor wording.
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

// local zone (which the daemon shares), so resolve the next occurrence locally.
function parseResetClock(screen: string, nowMs: number): number | null {
  const m = /\breset[s]?\b[^\n]{0,40}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(String(screen || ""));
  if (!m) return null;
  return usageParseTodayClock(m[1], nowMs);
}

interface ClarifyScore { confidence: number; questions: string[] }

function scoreClarifyingQuestion(screen: string): ClarifyScore {
  const block = activeClarifyingPromptBlock(screen);
  const options = block?.options ?? [];
  const questions = block?.questions ?? [];
  const sigMenu = options.length >= 2;                                  // 0.35
  const sigQuestion = questions.length > 0;                             // 0.30
  const sigConfirmFooter = !!block;                                     // 0.35

  let confidence = 0;
  if (sigMenu) confidence += 0.35;
  if (sigQuestion) confidence += 0.3;
  if (sigConfirmFooter) confidence += 0.35;
  if (confidence > 1) confidence = 1;
  return { confidence: confidence, questions: questions };
}

interface ClarifyingPromptBlock {
  questions: string[];
  options: ClaudeMenuOption[];
}

interface ActiveTerminalChoiceBlock {
  footerIdx: number;
  options: ClaudeMenuOption[];
}

function activeTerminalChoiceBlock(screen: string): ActiveTerminalChoiceBlock | null {
  const lines = String(screen || "").split("\n");
  for (let footerIdx = lines.length - 1; footerIdx >= 0; footerIdx--) {
    if (!isActiveChoiceFooter(lines[footerIdx])) continue;
    if (lastMeaningfulLineIndex(lines) > footerIdx) return null;

    const options = contiguousOptionsAbove(lines, footerIdx);
    if (options.length >= 2 && options.some((option) => option.highlighted)) {
      return { footerIdx, options };
    }
  }
  return null;
}

function activeClarifyingPromptBlock(screen: string): ClarifyingPromptBlock | null {
  const lines = String(screen || "").split("\n");
  const choice = activeTerminalChoiceBlock(screen);
  if (!choice || !isClarifyingConfirmFooter(lines[choice.footerIdx])) return null;
  const questions = questionsAbove(lines, choice.footerIdx);
  if (questions.length === 0) {
    const prompt = promptAboveOptions(lines, choice.footerIdx);
    if (prompt) questions.push(prompt);
  }
  return questions.length > 0 ? { questions, options: choice.options } : null;
}

function isActiveChoiceFooter(line: string): boolean {
  return /\b(?:press\s+)?enter\s+to\b/i.test(line);
}

function isClarifyingConfirmFooter(line: string): boolean {
  return /\bto (?:confirm|submit|select)\b/i.test(line);
}

function lastMeaningfulLineIndex(lines: string[]): number {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line) continue;
    if (isDividerLine(line)) continue;
    return i;
  }
  return -1;
}

function isDividerLine(line: string): boolean {
  const stripped = line.replace(/\s/g, "");
  return stripped.length >= 8 && /^[─━═-]+$/.test(stripped);
}

function contiguousOptionsAbove(lines: string[], footerIdx: number): ClaudeMenuOption[] {
  const options: ClaudeMenuOption[] = [];
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line) {
      if (options.length > 0) break;
      continue;
    }
    const m = CLAUDE_MENU_OPTION_RE.exec(lines[i]);
    if (!m) {
      if (options.length > 0) break;
      continue;
    }
    options.push({ highlighted: !!m[1], label: m[3] });
  }
  return options.reverse();
}

function promptAboveOptions(lines: string[], footerIdx: number): string | null {
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || CLAUDE_MENU_OPTION_RE.test(lines[i]) || isDividerLine(line)) continue;
    return line;
  }
  return null;
}

// Lines ending in a question mark above the active option block — the surfaced payload.
function questionsAbove(lines: string[], footerIdx: number): string[] {
  const out: string[] = [];
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line.length > 1 && line.charAt(line.length - 1) === "?") out.push(line);
    if (out.length > 0 && CLAUDE_MENU_OPTION_RE.test(line)) continue;
    if (out.length > 0 && !line) break;
  }
  return out.reverse();
}

// Auth prose is scored only inside the structurally active choice labels.
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
