// Mirrored by the native provider in agent_claude/provider/permission_prompt.rs; parity-tested.
import { buildClaudePromptResponse } from "./prompts";

export const PERMISSION_PROMPT_RULE = "claude.tool-permission.yes-once";

// Tools whose "permission" is a question or plan decision owed to the user.
const USER_DECISION_TOOLS = ["AskUserQuestion", "ExitPlanMode"];

interface PromptShape {
  question?: unknown;
  options?: unknown;
  sections?: unknown;
  multiSelect?: unknown;
  multi_select?: unknown;
  submitAvailable?: unknown;
  submit_available?: unknown;
}

export interface PermissionPromptResponse {
  optionIndex: number;
  rule: string;
  steps: string[];
}

function optionLabel(option: unknown): string {
  if (Array.isArray(option)) return String(option[0] ?? "");
  if (option && typeof option === "object") return String((option as { label?: unknown }).label ?? "");
  return "";
}

function trimFrame(text: string): string {
  return String(text || "").replace(/^[\s│┃]+|[\s│┃]+$/g, "");
}

function isDecline(label: string): boolean {
  const lower = label.toLowerCase();
  return lower === "no" || lower.startsWith("no,") || lower.startsWith("no ");
}

export function claudeAffirmativeOption(toolName: string, prompt: unknown, screen: string): number | null {
  const tool = String(toolName || "").trim();
  if (!tool || USER_DECISION_TOOLS.indexOf(tool) !== -1) return null;
  if (!prompt || typeof prompt !== "object") return null;
  const p = prompt as PromptShape;
  if (p.multiSelect === true || p.multi_select === true) return null;
  if (p.submitAvailable === true || p.submit_available === true) return null;
  if (Array.isArray(p.sections) && p.sections.length > 0) return null;
  const question = trimFrame(String(p.question ?? ""));
  const view = String(screen || "");
  if (!question.endsWith("?") || question.toLowerCase().indexOf("trust") !== -1 || view.indexOf(question) === -1) {
    return null;
  }
  const labels = (Array.isArray(p.options) ? p.options : []).map((option) => trimFrame(optionLabel(option)));
  if (labels.length < 2 || labels[0].toLowerCase() !== "yes") return null;
  return labels.slice(1).some(isDecline) ? 0 : null;
}

export function claudePermissionPromptResponse(
  toolName: string,
  prompt: unknown,
  screen?: string,
): PermissionPromptResponse | null {
  const view = String(screen || "");
  const optionIndex = claudeAffirmativeOption(toolName, prompt, view);
  if (optionIndex === null) return null;
  return {
    optionIndex,
    rule: PERMISSION_PROMPT_RULE,
    steps: buildClaudePromptResponse(optionIndex, view),
  };
}
