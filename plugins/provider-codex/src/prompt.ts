import type { ParsedPrompt } from "@codeterm/plugin-sdk";

const DOWN = "G1tC";
const UP = "G1tB";
const ENTER = "DQ==";

export interface MenuOption {
  highlighted: boolean;
  label: string;
  value: string;
  line: number;
}

export interface ActiveTerminalChoice {
  question: string;
  options: MenuOption[];
}

export type CodexFolderTrustPrompt =
  | {
      kind: "active";
      prompt: ParsedPrompt;
      affirmativeIndex: number;
      affirmativeSelected: boolean;
    }
  | { kind: "not_on_screen" };

const MENU_OPTION_RE = /^[ \t]*([>❯▶➤›])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;

export function parseCodexPrompt(screen: string): ParsedPrompt | null {
  const choice = parseActiveTerminalChoice(screen);
  return choice ? {
    question: choice.question,
    options: choice.options.map((option) => [option.label, option.value]),
  } : null;
}

export function parseCodexFolderTrustPrompt(screen: string): ParsedPrompt | null {
  const trust = classifyCodexFolderTrustPrompt(screen);
  return trust?.kind === "active" ? trust.prompt : null;
}

export function classifyCodexFolderTrustPrompt(
  screen: string,
): CodexFolderTrustPrompt | null {
  const options = activeMenuOptions(screen);
  if (options.length !== 2) return null;
  if (!hasDirectoryContextLine(screen, options[0].line)) return null;
  const affirmativeIndex = options.findIndex((option) => /^(?:yes|trust|allow)\b/i.test(option.label));
  const negativeIndex = options.findIndex((option) => /^(?:no|quit|cancel|decline|deny)\b/i.test(option.label));
  if (affirmativeIndex < 0 || negativeIndex < 0) return null;

  const preamble = cleanPromptText(menuPreamble(screen, options[0].line));
  if (!hasDirectoryTrustContent(preamble)) return null;
  const questionEnd = preamble.indexOf("?");
  const explanation = questionEnd < 0 ? "" : preamble.slice(questionEnd + 1).trim();
  if (explanation.split(/\s+/).filter(Boolean).length < 8) return null;
  if (!hasConfirmFooter(screen, options[options.length - 1].line)) return null;
  const choice = parseActiveTerminalChoice(screen);
  if (
    !choice ||
    choice.options.length !== 2 ||
    choice.options.filter((option) => option.highlighted).length !== 1
  ) {
    return { kind: "not_on_screen" };
  }

  return {
    kind: "active",
    affirmativeIndex,
    affirmativeSelected: choice.options[affirmativeIndex].highlighted,
    prompt: {
      question: preamble.slice(0, questionEnd + 1).trim(),
      options: choice.options.map((option) => [option.label, option.value] as [string, string]),
      multiSelect: false,
      submitAvailable: false,
    },
  };
}

function hasConfirmFooter(screen: string, lastOptionLine: number): boolean {
  const lines = normalizeLines(screen);
  let footerLine = -1;
  for (let i = lastOptionLine + 1; i < Math.min(lines.length, lastOptionLine + 5); i++) {
    if (/\benter\b.{0,24}\b(?:confirm|continue|select)\b/i.test(lines[i])) {
      footerLine = i;
      break;
    }
  }
  return footerLine >= 0 && lastMeaningfulLineIndex(lines) === footerLine;
}

function hasDirectoryContextLine(screen: string, firstOptionLine: number): boolean {
  const lines = normalizeLines(screen);
  const start = Math.max(0, firstOptionLine - 12);
  const context = lines.slice(start, firstOptionLine).map((line) => line.trim()).filter(Boolean).join(" ");
  const questionEnds: number[] = [];
  const questionPattern = /\?/g;
  let questionMatch: RegExpExecArray | null;
  while ((questionMatch = questionPattern.exec(context)) !== null) questionEnds.push(questionMatch.index);
  for (let i = questionEnds.length - 1; i >= 0; i--) {
    const questionEnd = questionEnds[i];
    const prefix = context.slice(0, questionEnd + 1);
    if (!/\b(folder|directory|workspace)\b/i.test(prefix)) continue;
    if (!/\b(trust|load(?:ed|ing)?)\b/i.test(prefix)) continue;
    if (prefix.split(/\s+/).some((token) => isAbsolutePathToken(token))) return true;
  }
  return false;
}

function hasDirectoryTrustContent(preamble: string): boolean {
  const content = preamble.toLowerCase();
  if (/\b(?:bypass|disable|override|without restrictions)\b/.test(content)) return false;
  const questionEnd = content.indexOf("?");
  if (questionEnd < 0) return false;
  const question = content.slice(0, questionEnd);
  const explanation = content.slice(questionEnd + 1);
  return (
    /\b(?:folder|directory|workspace)\b/.test(question) &&
    /\b(?:trust(?:ed|ing)?|load(?:ed|ing)?)\b/.test(question) &&
    /\b(?:guidance|rules|config(?:uration)?|settings|policy|policies|instructions|hooks)\b/.test(explanation)
  );
}

function isAbsolutePathToken(token: string): boolean {
  const path = token.replace(/[),;]+$/, "");
  return /^(?:[a-z]:[\\/]|\\\\|\/\/|\/)/i.test(path);
}

export function responseKeysForIndex(optionIndex: number, screen: string): string[] {
  const options = activeMenuOptions(screen);
  let highlightIdx = 0;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) { highlightIdx = i; break; }
  }
  const target = optionIndex > 0 ? optionIndex : 0;
  const delta = target - highlightIdx;
  const steps: string[] = [];
  if (delta > 0) for (let i = 0; i < delta; i++) steps.push(DOWN);
  else if (delta < 0) for (let i = 0; i < -delta; i++) steps.push(UP);
  steps.push(ENTER);
  return steps;
}

export function parseMenuOptions(screen: string): MenuOption[] {
  const lines = normalizeLines(screen);
  const options: MenuOption[] = [];
  for (let i = 0; i < lines.length; i++) {
    const m = MENU_OPTION_RE.exec(lines[i]);
    if (!m) continue;
    const indent = leadingIndent(lines[i]);
    const wrapped: string[] = [];
    for (let j = i + 1; j < lines.length; j++) {
      if (!lines[j].trim() || MENU_OPTION_RE.test(lines[j]) || isPromptFooter(lines[j])) break;
      if (leadingIndent(lines[j]) <= indent) break;
      wrapped.push(lines[j]);
    }
    options.push({
      highlighted: !!m[1],
      label: wrapped.reduce(joinWrappedLabel, m[3].trim()),
      value: m[2],
      line: i,
    });
  }
  return options;
}

function joinWrappedLabel(label: string, row: string): string {
  const continuation = row.trim();
  const pathBoundary = /[\\/]$/.test(label);
  return `${label}${pathBoundary ? "" : " "}${continuation}`;
}

export function parseActiveTerminalChoice(screen: string): ActiveTerminalChoice | null {
  const lines = normalizeLines(screen);
  const options = activeMenuOptions(screen);
  if (options.length < 2 || options.filter((option) => option.highlighted).length !== 1) return null;

  const lastOptionLine = options[options.length - 1].line;
  let footerLine = -1;
  for (let i = lastOptionLine + 1; i < Math.min(lines.length, lastOptionLine + 5); i++) {
    if (isActiveChoiceFooter(lines[i])) {
      footerLine = i;
      break;
    }
  }
  if (footerLine === -1 || lastMeaningfulLineIndex(lines) > footerLine) return null;
  return { question: questionForMenu(lines, options[0].line), options };
}

// The paragraph directly above a menu, joined across terminal wraps.
export function menuPreamble(screen: string, firstOptionLine: number): string {
  const lines = normalizeLines(screen);
  let i = firstOptionLine - 1;
  while (i >= 0 && !lines[i].trim()) i--;
  const parts: string[] = [];
  while (i >= 0 && lines[i].trim()) {
    parts.unshift(lines[i].trim());
    i--;
  }
  return parts.join(" ");
}

function activeMenuOptions(screen: string): MenuOption[] {
  const lines = normalizeLines(screen);
  const all = parseMenuOptions(screen);
  if (all.length < 2) return [];
  const cluster: MenuOption[] = [];
  let prev = all[all.length - 1].line;
  for (let i = all.length - 1; i >= 0; i--) {
    const between = lines.slice(all[i].line + 1, prev);
    const wrapsPreviousOption = between.every((line) =>
      line.trim().length > 0 && leadingIndent(line) > leadingIndent(lines[all[i].line]),
    );
    if (wrapsPreviousOption || cluster.length === 0) {
      cluster.unshift(all[i]);
      prev = all[i].line;
    } else {
      break;
    }
  }
  return cluster.length >= 2 ? cluster : [];
}

function isPromptFooter(line: string): boolean {
  return isActiveChoiceFooter(line);
}

function leadingIndent(line: string): number {
  return /^\s*/.exec(line)?.[0].length ?? 0;
}

function isActiveChoiceFooter(line: string): boolean {
  if (isActiveChoiceEnterFooter(line)) return true;
  const bindings = line.trim().split(/\s+[·•]\s+/);
  if (bindings.length < 2) return false;
  const keys = bindings.map((binding) => /^(enter|esc|[a-z])\s+\S.*$/i.exec(binding)?.[1]?.toLowerCase());
  return keys.every((key) => key !== undefined) && keys.includes("enter") && keys.includes("esc");
}

function isActiveChoiceEnterFooter(line: string): boolean {
  return /^(?:\S+[ \t]+)?enter[ \t]+\S.*$/i.test(line.trim());
}

function questionForMenu(lines: string[], firstLine: number): string {
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 16; i--) {
    const t = lines[i].trim();
    if (t.length > 3 && t.charAt(t.length - 1) === "?") return t;
  }
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 3; i--) {
    const t = lines[i].trim();
    if (t.length > 3) return t;
  }
  return "Permission required";
}

function normalizeLines(text: string): string[] {
  return String(text || "").replace(/\r*\n|\r/g, "\n").split("\n");
}

function lastMeaningfulLineIndex(lines: string[]): number {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || /^[-─━═]{8,}$/.test(line.replace(/\s/g, ""))) continue;
    return i;
  }
  return -1;
}

function cleanPromptText(value: string): string {
  return String(value || "").replace(/^[•⏺]\s*/, "").replace(/─+/g, " ").replace(/\s+/g, " ").trim();
}
