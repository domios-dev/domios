import { modelSwitchModeSupported } from "../../lib/modelSwitch";
import type { PendingPrompt } from "../../lib/types";

export function approvalHotkey(prompt: PendingPrompt, code: string): string | undefined {
  if (prompt.multi_select || prompt.sections?.length || !/^Digit[1-9]$/.test(code)) return undefined;
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

export function approvalActions(prompt: PendingPrompt) {
  const known: Record<string, { label: string; rank: number }> = {
    accept: { label: "Approve", rank: 0 }, once: { label: "Approve", rank: 0 },
    decline: { label: "Decline", rank: 1 }, reject: { label: "Decline", rank: 1 },
    acceptForSession: { label: "Accept for session", rank: 2 }, always: { label: "Accept for session", rank: 2 },
    cancel: { label: "Cancel", rank: 3 },
  };
  return prompt.options.map((option, index) => ({ ...option, label: known[option.value]?.label ?? option.label,
    rank: known[option.value]?.rank ?? index, hotkey: index + 1 })).sort((a, b) => a.rank - b.rank);
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
