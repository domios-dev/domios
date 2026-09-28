import type { ParsedPrompt } from "@codeterm/plugin-sdk";
import { claudeFolderTrustHighlightedOptionIndex } from "./folderTrust";

const WIZARD_SUBMIT_VALUE = 9000;
const WIZARD_MULTI_MASK_BASE = 10000;

interface ParsedOptionState {
  label: string;
  value: string;
  highlighted: boolean;
  selected: boolean;
}

export function parseClaudeWizardPrompt(text: string): ParsedPrompt | null {
  const raw = String(text || "");
  if (raw.indexOf("Submit") === -1 || raw.indexOf("Enter to select") === -1) return null;
  const tabLine = findWizardTabLine(raw);
  if (!tabLine) return null;
  const sections = parseWizardSections(tabLine);
  if (sections.length < 2) return null;

  const body = wizardBody(raw, tabLine);
  const options = parseWizardOptions(body);
  if (options.length < 2) return null;
  const question = parseWizardQuestion(body) || "Choose an option";
  const multiSelect = options.some((opt) => opt.selected) || /\[[ xX]\]|☐|☑|☒/.test(body);

  return {
    question,
    options: options.map((opt) => [opt.label, opt.value]),
    sections,
    multiSelect,
    submitAvailable: tabLine.indexOf("Submit") !== -1,
    selectedValues: options.filter((opt) => opt.selected).map((opt) => opt.value),
  };
}

export function findWizardTabLine(text: string): string | null {
  const lines = String(text || "").split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (line.indexOf("Submit") !== -1 && (line.indexOf("☐") !== -1 || line.indexOf("✔") !== -1)) {
      return line;
    }
  }
  const m = /←[\s\S]{0,160}Submit\s*→/.exec(text);
  return m ? m[0] : null;
}

export function parseWizardSections(tabLine: string): Array<{ label: string; active?: boolean; completed?: boolean }> {
  const sections: Array<{ label: string; active?: boolean; completed?: boolean }> = [];
  const re = /([☐☑☒✔])\s*([^☐☑☒✔←→]+?)(?=[☐☑☒✔]|→|$)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(tabLine)) !== null) {
    const label = m[2].trim();
    if (!label || /^submit$/i.test(label)) continue;
    sections.push({
      label,
      completed: m[1] === "✔" || m[1] === "☑" || m[1] === "☒",
    });
  }
  const active = sections.findIndex((s) => !s.completed);
  if (active >= 0) sections[active].active = true;
  return sections;
}

export function wizardBody(text: string, knownTabLine?: string): string {
  const tabLine = knownTabLine || findWizardTabLine(text);
  if (!tabLine) return text;
  return text.slice(text.indexOf(tabLine) + tabLine.length);
}

export function parseWizardQuestion(body: string): string {
  const beforeOptions = body.split(/(?:❯\s*)?\d+\./)[0] || "";
  return beforeOptions
    .replace(/^[\s←→☐☑☒✔A-Za-z]*Submit\s*→?/, "")
    .replace(/─+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function parseWizardOptions(body: string): ParsedOptionState[] {
  const checkbox = parseCheckboxOptions(body);
  if (checkbox.length >= 2) return checkbox;
  return parseNumberedOptions(body);
}

export function parseNumberedOptions(text: string): ParsedOptionState[] {
  const options: ParsedOptionState[] = [];
  const re = /^\s*[❯>]?\s*(\d+)\.[ ]+([^\r\n]*)/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const label = cleanOptionLabel(m[2]);
    if (!label) continue;
    options.push({
      label,
      value: m[1],
      highlighted: m[0].indexOf("❯") !== -1 || m[0].indexOf(">") !== -1,
      selected: false,
    });
  }
  return options;
}

export function parseCheckboxOptions(text: string): ParsedOptionState[] {
  const options: ParsedOptionState[] = [];
  const re = /^\s*[❯>]?\s*((?:\[[ xX]\])|☐|☑|☒|✔)\s*([^\r\n]*)/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const label = cleanOptionLabel(m[2]);
    if (!label) continue;
    const mark = m[1];
    options.push({
      label,
      value: String(options.length + 1),
      highlighted: m[0].indexOf("❯") !== -1 || m[0].indexOf(">") !== -1,
      selected: mark === "☑" || mark === "☒" || mark === "✔" || /\[[xX]\]/.test(mark),
    });
  }
  return options;
}

export function cleanOptionLabel(value: string): string {
  return String(value || "")
    .replace(/─+/g, " ")
    .replace(/\bChat\s*about\s*this\b[\s\S]*$/i, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function parseClaudeNumberedPrompt(text: string): ParsedPrompt | null {
  const raw = String(text || "");
  if (!/(Esc\s*to\s*cancel|Enter\s*to\s*(?:confirm|select)|Tab\s*to\s*amend|❯)/i.test(raw)) return null;
  const options = parseNumberedOptions(raw);
  if (options.length < 2) return null;
  const firstOption = raw.search(/(?:^|[\s❯>])\d+\.\s*/);
  const before = firstOption >= 0 ? raw.slice(0, firstOption) : raw;
  const question = findQuestion(before) || "Choose an option";
  return {
    question,
    options: options.map((opt) => [opt.label, opt.value]),
  };
}

export function parseClaudeLetteredPrompt(text: string): ParsedPrompt | null {
  const lines = String(text || "").replace(/\r\n?/g, "\n").split("\n");
  const optionStarts: Array<{ index: number; letter: string; label: string }> = [];
  const optionRe = /^\s*(?:[-*]\s*)?([A-Z])[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m || !/[A-C]/.test(m[1])) continue;
    optionStarts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (optionStarts.length < 3) return null;

  const options: ParsedOptionState[] = [];
  for (let i = 0; i < optionStarts.length; i++) {
    const start = optionStarts[i];
    const end = i + 1 < optionStarts.length ? optionStarts[i + 1].index : lines.length;
    const parts = [start.label];
    for (let j = start.index + 1; j < end; j++) {
      const t = lines[j].trim();
      if (!t) break;
      if (isClaudeFooterLine(t)) break;
      if (/^Step\s+\d+\s+of\s+\d+\b/i.test(t)) break;
      if (t.endsWith("?")) break;
      parts.push(t);
    }
    options.push({
      label: cleanOptionLabel(parts.join(" ")),
      value: start.letter,
      highlighted: false,
      selected: false,
    });
  }

  const groups = groupLetteredOptions(lines, optionStarts);
  const question = groups.length > 1
    ? findCompoundTitle(lines, groups[0].questionIndex) || groups[0].question || "Choose an option"
    : groups[0]?.question || findQuestion(lines.slice(0, optionStarts[0].index).join("\n")) || "Choose an option";
  const sections = groups.length > 1
    ? groups.map((group, i) => ({
        label: group.title || "Step " + String(i + 1),
        active: i === 0,
        completed: false,
      }))
    : undefined;

  return {
    question,
    options: options.map((opt) => [opt.label, opt.value]),
    sections,
    multiSelect: false,
    submitAvailable: false,
  };
}

export function isClaudeFooterLine(line: string): boolean {
  return line.indexOf("Baked for") !== -1
    || line.indexOf("Cogitated for") !== -1
    || line.indexOf("Spelunking") !== -1
    || line.indexOf("auto mode on") !== -1
    || line.indexOf("────────────────") !== -1
    || line === "❯";
}

export function groupLetteredOptions(
  lines: string[],
  starts: Array<{ index: number; letter: string; label: string }>,
): Array<{ question: string; questionIndex: number; title: string | null }> {
  const groups: Array<{ question: string; questionIndex: number; title: string | null }> = [];
  for (let i = 0; i < starts.length; i++) {
    if (starts[i].letter !== "A") continue;
    const questionIndex = nearestQuestionIndex(lines, starts[i].index);
    const title = nearestSectionTitle(lines, questionIndex);
  groups.push({
      question: questionIndex >= 0 ? cleanClaudeLine(lines[questionIndex]) : "",
      questionIndex,
      title,
    });
  }
  return groups;
}

export function nearestQuestionIndex(lines: string[], beforeIndex: number): number {
  const start = Math.max(0, beforeIndex - 8);
  for (let i = beforeIndex - 1; i >= start; i--) {
    const t = lines[i].trim();
    if (t.endsWith("?")) return i;
  }
  return -1;
}

export function nearestSectionTitle(lines: string[], questionIndex: number): string | null {
  if (questionIndex < 0) return null;
  for (let i = questionIndex - 1; i >= Math.max(0, questionIndex - 5); i--) {
    const t = cleanClaudeLine(lines[i]);
    if (!t) continue;
    const step = /^Step\s+\d+\s+of\s+\d+\s+[—-]\s+(.+)$/i.exec(t);
    if (step && step[1]) return step[1].trim();
    if (!t.endsWith("?") && !/^[❯>]/.test(t)) return t;
  }
  return null;
}

export function findCompoundTitle(lines: string[], beforeQuestionIndex: number): string | null {
  if (beforeQuestionIndex < 0) return null;
  for (let i = beforeQuestionIndex - 1; i >= Math.max(0, beforeQuestionIndex - 8); i--) {
    const t = cleanClaudeLine(lines[i]);
    if (!t) continue;
    if (/^Step\s+\d+\s+of\s+\d+\b/i.test(t)) continue;
    if (t.endsWith("?") || /^[❯>]/.test(t)) continue;
    return t;
  }
  return null;
}

export function cleanClaudeLine(line: string): string {
  return String(line || "").trim().replace(/^⏺\s*/, "").trim();
}

export function findQuestion(text: string): string {
  const lines = String(text || "").split("\n").map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].endsWith("?")) return lines[i];
  }
  return lines.length > 0 ? lines[lines.length - 1] : "";
}

export function buildClaudePromptResponse(optionIndex: number, screen: string): string[] {
  if (optionIndex === WIZARD_SUBMIT_VALUE - 1) return buildWizardSubmitResponse(screen);
  if (optionIndex >= WIZARD_MULTI_MASK_BASE - 1) {
    return buildWizardMultiSelectResponse(optionIndex - (WIZARD_MULTI_MASK_BASE - 1), screen);
  }
  const steps = moveToOption(optionIndex, screen);
  steps.push("DQ==");
  if (parseClaudeWizardPrompt(screen) && !parseClaudeWizardPrompt(screen)?.multiSelect) steps.push("CQ==");
  return steps;
}

export interface ClaudeOnboardingPrompt {
  question: string;
  options: [string, string][];
  frameKey: string;
  step: string;
}

interface OnboardingOption {
  focused: boolean;
  number: string;
  label: string;
  line: number;
}

export function claudeOnboardingPrompt(screen: string): ClaudeOnboardingPrompt | null {
  const lines = String(screen || "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .split(/\r?\n/)
    .map(onboardingRow);
  const candidates = lines.flatMap((line, index) => {
    const match = /^\s*([❯>])?\s*(\d+)\.[ ]+(.*?)\s*$/.exec(line);
    return match ? [{ focused: Boolean(match[1]), number: match[2], label: match[3].trim(), line: index }] : [];
  });
  const options = candidates.slice(-3);
  if (options.length !== 3 || options.some((option, index) => option.number !== String(index + 1))) return null;
  if (options.filter((option) => option.focused).length !== 1) return null;
  const first = options[0].line;
  const last = options[2].line;
  if (last - first > 4) return null;
  const footer = lines.slice(last + 1, last + 4).join(" ").toLowerCase();
  if (!footer.includes("enter") || !footer.includes("esc")) return null;
  let question = "";
  for (let index = first - 1; index >= Math.max(0, first - 5); index--) {
    if (lines[index].endsWith("?")) {
      question = lines[index];
      break;
    }
  }
  const questionWords = question.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  if (!questionWords.includes("auto") || !questionWords.includes("environment")) return null;
  const labels = options.map((option) => normalizeOnboardingLabel(option.label));
  const target = labels.findIndex((label) => label.startsWith("not now"));
  if (target < 0 || labels.filter((label) => label.startsWith("yes")).length !== 1) return null;
  if (labels.filter((label) => label.startsWith("don't show again")).length !== 1) return null;
  const current = options.findIndex((option) => option.focused);
  const step = current === target ? "DQ==" : target > current ? "ag==" : "aw==";
  return {
    question,
    options: options.map((option) => [option.label, option.number]),
    frameKey: `onboarding:focus-${current}:target-${target}`,
    step,
  };
}

export function claudeOnboardingNotNowStep(screen: string): { frameKey: string; step: string } | null {
  const prompt = claudeOnboardingPrompt(screen);
  return prompt ? { frameKey: prompt.frameKey, step: prompt.step } : null;
}

function onboardingRow(row: string): string {
  return row
    .replace(/^\s*[│┃║]\s*/, "")
    .replace(/\s*[│┃║]\s*$/, "")
    .trim();
}

function normalizeOnboardingLabel(label: string): string {
  return label.toLowerCase().replace(/[’‘]/g, "'").trim();
}

export function moveToOption(targetIndex: number, screen: string): string[] {
  const current = currentHighlightedOptionIndex(screen);
  const delta = targetIndex - current;
  const steps: string[] = [];
  const key = delta >= 0 ? "ag==" : "aw==";
  for (let i = 0; i < Math.abs(delta); i++) steps.push(key);
  return steps;
}

export function currentHighlightedOptionIndex(screen: string): number {
  const trustIndex = claudeFolderTrustHighlightedOptionIndex(screen);
  if (trustIndex !== null) return trustIndex;
  const body = wizardBody(screen);
  const numbered = parseNumberedOptions(body);
  const options = numbered.length >= 2 ? numbered : parseWizardOptions(body);
  const idx = options.findIndex((opt) => opt.highlighted);
  return idx >= 0 ? idx : 0;
}

export function buildWizardMultiSelectResponse(mask: number, screen: string): string[] {
  const options = parseWizardOptions(wizardBody(screen));
  const steps: string[] = [];
  let current = currentHighlightedOptionIndex(screen);
  for (let i = 0; i < options.length; i++) {
    const desired = (mask & (1 << i)) !== 0;
    if (options[i].selected === desired) continue;
    const delta = i - current;
    const key = delta >= 0 ? "ag==" : "aw==";
    for (let j = 0; j < Math.abs(delta); j++) steps.push(key);
    steps.push("IA==");
    current = i;
  }
  steps.push("CQ==");
  return steps;
}

export function buildWizardSubmitResponse(screen: string): string[] {
  const parsed = parseClaudeWizardPrompt(screen);
  const steps: string[] = [];
  const active = parsed?.sections?.findIndex((section: { active?: boolean }) => section.active) ?? -1;
  const count = parsed?.sections?.length ?? 0;
  const moves = active >= 0 && count > active ? count - active : 1;
  for (let i = 0; i < moves; i++) steps.push("G1tD");
  steps.push("DQ==");
  return steps;
}
