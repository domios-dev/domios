import { modelSwitchModeSupported } from "../../lib/modelSwitch";
import type { PendingPrompt } from "../../lib/types";

export function approvalHotkey(prompt: PendingPrompt, code: string): string | undefined {
  if (prompt.multi_select || prompt.sections?.length || prompt.elicitation?.mode === "form" || !/^Digit[1-9]$/.test(code)) return undefined;
  return prompt.options[Number(code.slice(5)) - 1]?.value;
}

export function contextMeter(value: unknown): { used: number; window: number; percent: number } | null {
  if (!value || typeof value !== "object") return null;
  const data = value as Record<string, unknown>;
  const used = data.used_tokens;
  const window = data.window_tokens;
  if (typeof used !== "number" || typeof window !== "number" || !Number.isFinite(used) || !Number.isFinite(window) || !Number.isInteger(used) || !Number.isInteger(window) || used < 0 || window <= 0) return null;
  return { used, window, percent: Math.min(100, used / window * 100) };
}

export function rememberPrompt(history: readonly string[], text: string): string[] {
  if (!text.trim() || history[history.length - 1] === text) return [...history];
  return [...history, text].slice(-50);
}

export function swapStash(draft: string, stashed: string): { draft: string; stashed: string } {
  return { draft: stashed, stashed: draft };
}

export type ApprovalTone = "primary" | "ghost" | "danger";

export function approvalActions(prompt: PendingPrompt) {
  const known: Record<string, { label: string; rank: number; tone: ApprovalTone }> = {
    allow_once: { label: "Allow once", rank: 0, tone: "primary" },
    accept: { label: "Allow once", rank: 0, tone: "primary" },
    once: { label: "Allow once", rank: 0, tone: "primary" },
    allow_always: { label: "Always allow", rank: 1, tone: "ghost" },
    acceptForSession: { label: "Always allow", rank: 1, tone: "ghost" },
    always: { label: "Always allow", rank: 1, tone: "ghost" },
    reject_once: { label: "Reject", rank: 2, tone: "danger" },
    reject: { label: "Reject", rank: 2, tone: "danger" },
    decline: { label: "Decline", rank: 2, tone: "danger" },
    reject_always: { label: "Always reject", rank: 3, tone: "danger" },
    cancel: { label: "Cancel", rank: 3, tone: "ghost" },
  };
  return prompt.options.map((option, index) => {
    const match = known[option.value];
    const lowerLabel = option.label.trim().toLowerCase();
    const inferredTone: ApprovalTone = match?.tone
      ?? (/reject|deny|decline|block/.test(lowerLabel) ? "danger" : index === 0 ? "primary" : "ghost");
    const inferredRank = match?.rank
      ?? (/allow once|approve|yes/.test(lowerLabel) ? 0 : /always allow|session/.test(lowerLabel) ? 1 : /reject|deny|decline/.test(lowerLabel) ? 2 : index + 3);
    const label = match && option.label.trim().toLowerCase() === option.value.toLowerCase()
      ? match.label
      : option.label.trim() || match?.label || option.value;
    return { ...option, label, rank: inferredRank, tone: inferredTone, hotkey: index + 1 };
  }).sort((a, b) => a.rank - b.rank);
}

export function mergeRuntimePromptDetail(
  prompt: PendingPrompt | null | undefined,
  detail: import("../../lib/agentRuntimeState").AgentRuntimeState["pendingPromptDetail"],
): PendingPrompt | null {
  if (!prompt) return null;
  if (!detail || (detail.requestId && prompt.request_id && detail.requestId !== prompt.request_id)) return prompt;
  return {
    ...prompt,
    ...(detail.fileDiffs?.length ? { file_diffs: detail.fileDiffs } : {}),
    ...(detail.elicitation ? { elicitation: detail.elicitation } : {}),
  };
}

export function permissionHeaderTitle(prompt: PendingPrompt): string {
  const lines = prompt.question
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => Boolean(line) && !/^Writes\s+.+\(\+\d+\/-\d+\s+lines\)$/.test(line) && !/^(Kind|Path|Diff):/i.test(line) && line !== "Allow this tool call?");
  const first = lines[0] ?? "";
  if (!first || /^request_permission$/i.test(first)) {
    return prompt.file_diffs?.length ? "Apply file edit" : "Permission requested";
  }
  return first;
}

export interface RecallPosition { index: number; draft: string }
export function recallPrompt(history: readonly string[], position: RecallPosition | null, draft: string, direction: -1 | 1) {
  const current = position ?? { index: history.length, draft };
  const index = Math.max(0, Math.min(history.length, current.index + direction));
  return { position: { ...current, index }, text: history[index] ?? current.draft };
}

export function createApprovalLatch() {
  let answered = false;
  return { claim() { if (answered) return false; answered = true; return true; } };
}

export function composerPickerState(modelSwitch: string | undefined, permissionBypass: boolean | undefined) {
  return { modelEnabled: modelSwitchModeSupported(modelSwitch), fastEnabled: false,
    runtimeMode: permissionBypass ? "launch_flag" as const : "unavailable" as const };
}
