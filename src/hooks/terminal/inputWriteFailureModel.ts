import { terminalInputTarget } from "./terminalInputWrite";

export type InputWriteFailureState = string | null;
export type InputWriteFailureAction =
  | { kind: "failure" | "success"; targetId: string }
  | { kind: "dismiss" };

export function inputWriteFailureTarget(tabId: string, ptyId: string | null): string | null {
  return ptyId ? terminalInputTarget(tabId, ptyId) : null;
}

export function reduceInputWriteFailure(
  state: InputWriteFailureState,
  targetId: string | null,
  action: InputWriteFailureAction,
): InputWriteFailureState {
  const current = state === targetId ? state : null;
  if (action.kind === "dismiss") return null;
  if (!targetId || action.targetId !== targetId) return current;
  return action.kind === "failure" ? targetId : null;
}
