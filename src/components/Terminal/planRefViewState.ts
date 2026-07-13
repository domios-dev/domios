export type PlanRefViewState =
  | "resolved"
  | "archived"
  | "unowned"
  | "stale"
  | "missing"
  | "loading"
  | "unreachable"
  | "permission";

const TERMINAL_BACKEND_STATES = new Set<PlanRefViewState>([
  "missing",
  "permission",
  "unreachable",
]);

export function planRefViewState(
  backendState: PlanRefViewState,
  hasOwnerWorkspace: boolean,
  resolverUnreachable: boolean,
): PlanRefViewState {
  if (resolverUnreachable) return "unreachable";
  if (TERMINAL_BACKEND_STATES.has(backendState)) return backendState;
  return hasOwnerWorkspace ? backendState : "unowned";
}
