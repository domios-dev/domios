import { parseActiveTerminalChoice } from "./prompt";

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

const CLASSIFY_THRESHOLD = 0.8;
const RESUME_BUFFER_MS = 60 * 1000;

export function classifyCodexTabState(
  snapshot: TabStateSnapshot,
  nowMs: number,
): TabStateClassification | null {
  const screen = String((snapshot && snapshot.screenText) || "").replace(/\r\n?/g, "\n");
  if (!screen.trim()) return null;
  if (looksBusy(screen)) return { state: "working", confidence: 0 };

  const activeChoice = parseActiveTerminalChoice(screen);
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
  if (looksLikeModelPicker(screen)) return { state: "working", confidence: 0 };

  const update = scoreUpdatePrompt(screen);
  if (update.confidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "update_prompt",
      confidence: update.confidence,
    };
  }

  const rateLimit = scoreRateLimit(screen, nowMs);
  if (rateLimit.confidence >= CLASSIFY_THRESHOLD) {
    const action: TabStateAction = {};
    const data: { resetAt?: string } = {};
    if (rateLimit.resetAtMs !== null) {
      data.resetAt = new Date(rateLimit.resetAtMs).toISOString();
      action.delayedResume = {
        at: new Date(rateLimit.resetAtMs + RESUME_BUFFER_MS).toISOString(),
        text: "continue your work",
        thenKeys: ["enter"],
      };
    } else {
      action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
    }
    return {
      state: "rate_limited",
      confidence: rateLimit.confidence,
      data: rateLimit.resetAtMs !== null ? data : undefined,
      action,
    };
  }

  if (activeChoice) {
    return {
      state: "clarifying_question",
      confidence: 1,
      data: { questions: [activeChoice.question] },
      action: {
        surface: {
          kind: "clarifying_question",
          payload: { questions: [activeChoice.question] },
        },
      },
    };
  }

  const partial = rateLimit.confidence > update.confidence
    ? rateLimit.confidence
    : update.confidence;
  if (partial > 0) return { state: "unknown", confidence: partial };
  return { state: "working", confidence: 0 };
}

interface UpdateScore {
  confidence: number;
}

function scoreUpdatePrompt(screen: string): UpdateScore {
  const options = parseActiveTerminalChoice(screen)?.options ?? [];
  let hasUpdateOption = false;
  let hasSkipOption = false;
  for (let i = 0; i < options.length; i++) {
    if (/\bupdate\b/i.test(options[i].label)) hasUpdateOption = true;
    if (/\bskip\b/i.test(options[i].label)) hasSkipOption = true;
  }
  const actionableMenu = hasUpdateOption && hasSkipOption;
  let confidence = 0;
  if (actionableMenu) confidence += 0.45;
  if (/\b(update|version)\s+available\b/i.test(screen)) confidence += 0.2;
  if (/\d+\.\d+(?:\.\d+)?\s*(?:->|→|=>)\s*\d+\.\d+(?:\.\d+)?/.test(screen)) {
    confidence += 0.15;
  }
  if (/press\s+enter\s+to\s+continue/i.test(screen)) confidence += 0.1;
  if (/release\s+notes/i.test(screen)) confidence += 0.1;
  if (confidence > 1) confidence = 1;
  return { confidence };
}

function looksBusy(screen: string): boolean {
  return (
    /\bctrl\+c to (?:cancel|stop|interrupt|quit)\b/i.test(screen)
    || /\bWorking[….]/.test(screen)
    || /\((?:esc|ctrl\+c) to interrupt\)/i.test(screen)
  );
}

function looksLikeModelPicker(screen: string): boolean {
  if (!/\bselect\s+(?:model|reasoning)\b/i.test(screen)) return false;
  return /\bpress\s+enter\s+to\s+confirm\b/i.test(screen)
    && /\besc\s+to\s+go\s+back\b/i.test(screen);
}

function scoreRateLimit(screen: string, nowMs: number) {
  let confidence = 0;
  if (/\bhit your usage limit\b/i.test(screen) || /\breached your usage limit\b/i.test(screen)) {
    confidence += 0.5;
  }
  if (/\btry again at\b/i.test(screen)) confidence += 0.3;
  if (/chatgpt\.com\/codex\/settings/i.test(screen)) confidence += 0.1;
  if (/\btry again at\b[^\n]{0,48}?\d{1,2}(?::\d{2})?\s*(?:am|pm)\b/i.test(screen)) {
    confidence += 0.1;
  }
  if (confidence > 1) confidence = 1;
  return { confidence, resetAtMs: parseTryAgainClock(screen, nowMs) };
}

function parseTryAgainClock(screen: string, nowMs: number): number | null {
  const match = /\btry again at\b[^\n]{0,48}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(screen);
  return match ? parseTodayClock(match[1], nowMs) : null;
}

function parseTodayClock(value: string, nowMs: number): number | null {
  const match = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)$/i.exec(value.trim());
  if (!match) return null;
  const hour = Number.parseInt(match[1], 10);
  const minute = Number.parseInt(match[2] || "0", 10);
  if (hour === 0 || hour > 12 || minute > 59) return null;
  const hour24 = (hour % 12) + (match[3].toLowerCase() === "pm" ? 12 : 0);
  const now = new Date(nowMs);
  let candidate = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, minute, 0, 0);
  if (candidate.getTime() <= nowMs) {
    candidate = new Date(candidate.getTime() + 24 * 3600 * 1000);
  }
  return candidate.getTime();
}

function scoreAuthPrompt(screen: string): number {
  let score = 0;
  if (/\bsign[ -]?in\b/i.test(screen)) score += 0.5;
  if (/\blog[ -]?in\b/i.test(screen)) score += 0.5;
  if (/\bauthenticat(?:e|ion|ing)\b/i.test(screen)) score += 0.5;
  if (/\bo\s?auth\b/i.test(screen)) score += 0.6;
  if (/\bapi key\b/i.test(screen)) score += 0.6;
  if (/paste\b[^\n]*\b(token|key|code|credential)/i.test(screen)) score += 0.6;
  if (/\bauthorization code\b/i.test(screen)) score += 0.6;
  if (/enter your\b[^\n]*\b(api key|token|credential)/i.test(screen)) score += 0.6;
  return score > 1 ? 1 : score;
}
