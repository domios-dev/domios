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

interface ParsedOptionState {
  label: string;
  value: string;
  highlighted: boolean;
}

interface PromptShape {
  prompt: ParsedPrompt;
  interactive: boolean;
}

const MENU_OPTION_RE = /^[ \t]*([>❯▶➤›])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;

export function parseCodexPrompt(screen: string): ParsedPrompt | null {
  const parsed = parseCodexPromptShape(String(screen || ""));
  return parsed ? parsed.prompt : null;
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
  const questionLine = lines.findIndex((line, index) => {
    if (index < start || index >= firstOptionLine || !line.includes("?")) return false;
    return /\b(folder|directory|workspace)\b/i.test(line)
      && /\b(trust|load(?:ed|ing)?)\b/i.test(line);
  });
  if (questionLine <= start) return false;
  return lines.slice(start, questionLine).some((line) => {
    const text = line.trim();
    const context = /^[>❯›][ \t]+(.+)$/.exec(text)?.[1] || text;
    return context.split(/\s+/).some((token) => isAbsolutePathToken(token));
  });
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
  const parsed = parseCodexPromptShape(screen);
  if (parsed && !parsed.interactive) {
    const options = (parsed.prompt as { options?: Array<{ value?: string } | [string, string]> }).options || [];
    const option = options[optionIndex];
    const value = Array.isArray(option) ? option[1] : option && option.value;
    return [base64(String(value || optionIndex + 1) + "\r")];
  }

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
    options.push({ highlighted: !!m[1], label: m[3].trim(), value: m[2], line: i });
  }
  return options;
}

export function parseActiveTerminalChoice(screen: string): ActiveTerminalChoice | null {
  const lines = normalizeLines(screen);
  const options = activeMenuOptions(screen);
  if (options.length < 2 || !options.some((option) => option.highlighted)) return null;

  const lastOptionLine = options[options.length - 1].line;
  let footerLine = -1;
  for (let i = lastOptionLine + 1; i < Math.min(lines.length, lastOptionLine + 5); i++) {
    if (/\benter\b.{0,24}\b(?:confirm|continue|select)\b/i.test(lines[i])) {
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

function parseCodexPromptShape(screen: string): PromptShape | null {
  return parseCodexNumberedMenu(screen)
    || parseCodexLetteredPrompt(screen)
    || parseCodexNumberedQuestion(screen);
}

function parseCodexNumberedMenu(screen: string): PromptShape | null {
  const lines = normalizeLines(screen);
  const options = activeMenuOptions(screen);
  if (options.length < 2) return null;
  const hasSelector = options.some((o) => o.highlighted);
  const hasFooter = promptFooterNear(lines, options[options.length - 1].line);
  if (!hasSelector && !hasFooter) return null;
  const question = questionForMenu(lines, options[0].line);
  return {
    prompt: { question: question, options: options.map((o) => [o.label, o.value]) },
    interactive: true,
  };
}

function activeMenuOptions(screen: string): MenuOption[] {
  const all = parseMenuOptions(screen);
  if (all.length < 2) return [];
  const cluster: MenuOption[] = [];
  let prev = all[all.length - 1].line;
  for (let i = all.length - 1; i >= 0; i--) {
    if (prev - all[i].line <= 2 || cluster.length === 0) {
      cluster.unshift(all[i]);
      prev = all[i].line;
    } else {
      break;
    }
  }
  return cluster.length >= 2 ? cluster : [];
}

function promptFooterNear(lines: string[], lastLine: number): boolean {
  for (let i = lastLine; i < Math.min(lines.length, lastLine + 5); i++) {
    const t = lines[i].toLowerCase();
    if (
      (t.indexOf("esc") !== -1 || t.indexOf("enter") !== -1) &&
      (t.indexOf("confirm") !== -1 || t.indexOf("go back") !== -1 ||
        t.indexOf("cancel") !== -1 || t.indexOf("continue") !== -1)
    ) return true;
  }
  return false;
}

function questionForMenu(lines: string[], firstLine: number): string {
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 5; i--) {
    const t = lines[i].trim();
    if (t.length > 3 && t.charAt(t.length - 1) === "?") return t;
  }
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 3; i--) {
    const t = lines[i].trim();
    if (t.length > 3) return t;
  }
  return "Permission required";
}

function parseCodexLetteredPrompt(text: string): PromptShape | null {
  const lines = prosePromptLines(text);
  const starts: Array<{ index: number; letter: string; label: string }> = [];
  const optionRe = /^\s*(?:[-*•]\s*)?([A-Z])[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m || !/[A-C]/.test(m[1])) continue;
    starts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (starts.length < 3) return null;
  return promptFromOptionStarts(lines, starts, "letter");
}

function parseCodexNumberedQuestion(text: string): PromptShape | null {
  const lines = prosePromptLines(text);
  const starts: Array<{ index: number; letter: string; label: string }> = [];
  const optionRe = /^\s*(?:[-*•]\s*)?(\d+)[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m) continue;
    starts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (starts.length < 2) return null;
  return promptFromOptionStarts(lines, starts, "number");
}

function promptFromOptionStarts(
  lines: string[],
  starts: Array<{ index: number; letter: string; label: string }>,
  kind: "letter" | "number",
): PromptShape | null {
  const options: ParsedOptionState[] = [];
  for (let i = 0; i < starts.length; i++) {
    const start = starts[i];
    const end = i + 1 < starts.length ? starts[i + 1].index : lines.length;
    const parts = [start.label];
    for (let j = start.index + 1; j < end; j++) {
      const t = lines[j].trim();
      if (!t || isCodexFooterLine(t) || /^Step\s+\d+\s+of\s+\d+\b/i.test(t) || t.endsWith("?")) break;
      parts.push(t);
    }
    options.push({ label: cleanPromptText(parts.join(" ")), value: start.letter, highlighted: false });
  }
  const groups = groupOptions(lines, starts, kind);
  if (groups.length === 0) return null;
  const question = groups.length > 1
    ? findCompoundTitle(lines, groups[0].questionIndex) || groups[0].question || "Choose an option"
    : groups[0].question || findQuestion(lines.slice(0, starts[0].index).join("\n")) || "Choose an option";
  const sections = groups.length > 1
    ? groups.map((group, i) => ({ label: group.title || "Step " + String(i + 1), active: i === 0 }))
    : undefined;
  return {
    prompt: {
      question,
      options: options.map((opt) => [opt.label, opt.value]),
      sections,
      multiSelect: false,
      submitAvailable: false,
    },
    interactive: false,
  };
}

function groupOptions(
  lines: string[],
  starts: Array<{ index: number; letter: string; label: string }>,
  kind: "letter" | "number",
): Array<{ question: string; questionIndex: number; title: string | null }> {
  const firstValue = kind === "letter" ? "A" : "1";
  const groups: Array<{ question: string; questionIndex: number; title: string | null }> = [];
  for (let i = 0; i < starts.length; i++) {
    if (starts[i].letter !== firstValue) continue;
    const questionIndex = nearestQuestionIndex(lines, starts[i].index);
    groups.push({
      question: questionIndex >= 0 ? cleanPromptText(lines[questionIndex]) : "",
      questionIndex,
      title: nearestSectionTitle(lines, questionIndex),
    });
  }
  return groups;
}

function normalizeLines(text: string): string[] {
  return String(text || "").replace(/\r\n?/g, "\n").split("\n");
}

function lastMeaningfulLineIndex(lines: string[]): number {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || /^[-─━═]{8,}$/.test(line.replace(/\s/g, ""))) continue;
    return i;
  }
  return -1;
}

function prosePromptLines(text: string): string[] {
  const lines = normalizeLines(text);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (/^\s*•\s+.+\?\s*$/.test(lines[i])) return lines.slice(i);
  }
  return lines;
}

function nearestQuestionIndex(lines: string[], beforeIndex: number): number {
  const start = Math.max(0, beforeIndex - 8);
  for (let i = beforeIndex - 1; i >= start; i--) {
    const t = lines[i].trim();
    if (t.endsWith("?")) return i;
  }
  return -1;
}

function nearestSectionTitle(lines: string[], questionIndex: number): string | null {
  if (questionIndex < 0) return null;
  for (let i = questionIndex - 1; i >= Math.max(0, questionIndex - 5); i--) {
    const t = cleanPromptText(lines[i]);
    if (!t || t.endsWith("?") || /^[❯>]/.test(t)) continue;
    const step = /^Step\s+\d+\s+of\s+\d+\s+[—-]\s+(.+)$/i.exec(t);
    return step && step[1] ? step[1].trim() : t;
  }
  return null;
}

function findCompoundTitle(lines: string[], beforeQuestionIndex: number): string | null {
  if (beforeQuestionIndex < 0) return null;
  for (let i = beforeQuestionIndex - 1; i >= Math.max(0, beforeQuestionIndex - 8); i--) {
    const t = cleanPromptText(lines[i]);
    if (!t || /^Step\s+\d+\s+of\s+\d+\b/i.test(t) || t.endsWith("?") || /^[❯>]/.test(t)) continue;
    return t;
  }
  return null;
}

function findQuestion(text: string): string {
  const lines = normalizeLines(text).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].endsWith("?")) return cleanPromptText(lines[i]);
  }
  return lines.length > 0 ? cleanPromptText(lines[lines.length - 1]) : "";
}

function isCodexFooterLine(line: string): boolean {
  return line.indexOf("ctrl+c to cancel") !== -1
    || line.indexOf("tokens used") !== -1
    || line.indexOf("────────────────") !== -1
    || line === "❯"
    || /^›\s*$/.test(line);
}

function cleanPromptText(value: string): string {
  return String(value || "").replace(/^[•⏺]\s*/, "").replace(/─+/g, " ").replace(/\s+/g, " ").trim();
}

function base64(value: string): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const bytes: number[] = [];
  for (let i = 0; i < value.length; i++) bytes.push(value.charCodeAt(i) & 0xff);
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i];
    const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
    out += chars[a >> 2];
    out += chars[((a & 3) << 4) | (b >> 4)];
    out += i + 1 < bytes.length ? chars[((b & 15) << 2) | (c >> 6)] : "=";
    out += i + 2 < bytes.length ? chars[c & 63] : "=";
  }
  return out;
}
