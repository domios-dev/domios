import type { AgentEvent, RuntimeTurnDetail } from "@codeterm/plugin-sdk";
import { object, list } from "./runtimeProtocol";

type TurnStateEvent = Extract<AgentEvent,{kind:"turnState"}>;
export type TurnStatus = Pick<TurnStateEvent,"state"|"detail">;

export function threadStatus(raw: unknown): TurnStatus | undefined {
  const status = object(raw);
  const type = typeof raw === "string" ? raw : status.type;
  if (type === "idle") return {state:"idle"};
  if (type === "systemError") return {state:"needs_input",detail:"systemError"};
  if (type !== "active") return undefined;
  const flags = list(status.activeFlags);
  if (flags.includes("waitingOnApproval")) return {state:"needs_input",detail:"awaitingApproval"};
  if (flags.includes("waitingOnUserInput")) return {state:"needs_input",detail:"awaitingInput"};
  return {state:"working"};
}

export function turnOutcome(turn: unknown): RuntimeTurnDetail | undefined {
  const status = object(turn).status;
  return status === "interrupted" ? "interrupted" : status === "failed" ? "failed" : undefined;
}

export function requestDetail(method: string): RuntimeTurnDetail {
  return method.endsWith("/requestApproval") ? "awaitingApproval" : "awaitingInput";
}

export function withDetail(status: TurnStatus, compacting: boolean): TurnStatus {
  return status.state === "working" && compacting ? {state:"working",detail:"compacting"} : status;
}
