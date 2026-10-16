import type { ChatSessionIntent } from "@codeterm/plugin-sdk";
import type { RuntimePlan, RuntimeStep, JsonValue, RpcReply } from "@codeterm/plugin-sdk";
const statuses = new Set(["active", "paused", "blocked", "usageLimited", "budgetLimited", "complete"]);
export function goalIntent(raw: unknown, thread?: string): ChatSessionIntent | null {
  if (!raw || typeof raw !== "object") return null;
  const goal = raw as Record<string, unknown>;
  if (thread && goal.threadId !== thread) return null;
  if (typeof goal.objective !== "string" || !goal.objective.trim()) return null;
  return {goal:goal.objective,...(typeof goal.status === "string" && statuses.has(goal.status) ? {goalStatus:goal.status} : {})};
}
export function goalPlan(threadId: string, value: JsonValue): RuntimePlan {
  const action = value && typeof value === "object" && !Array.isArray(value) ? value.action : undefined;
  const status = action === "pause" ? "paused" : action === "resume" ? "active" : action === "complete" ? "complete" : undefined;
  if (action !== "read" && action !== "cancel" && !status) throw new Error("Unsupported goal action");
  const method = action === "read" ? "thread/goal/get" : action === "cancel" ? "thread/goal/clear" : "thread/goal/set";
  return {action:{kind:"request",call:{method,params:{threadId,...(action !== "read" ? {origin:"user"} : {}),...(status ? {status} : {})}}},state:{goalAction:String(action)}};
}
export function goalResponse(threadId: string, state: Record<string, unknown>, reply: RpcReply): RuntimeStep {
  const refuse = (message:string):RuntimeStep => ({kind:"refused",reason:{code:"provider_refused",message},beforeAcceptance:false});
  if (reply.kind === "error") return refuse(reply.error.message);
  const value = reply.value as Record<string, unknown> | null;
  if (!value || typeof value !== "object") return refuse("Goal response is invalid");
  if (state.goalAction === "cancel") return value.cleared === true
    ? {kind:"continue",plan:{...goalPlan(threadId,{action:"read"}),state:{goalAction:"verifyCancel"}}} : refuse("Goal removal was not confirmed");
  if (!("goal" in value)) return refuse("Goal response is missing");
  if (state.goalAction === "verifyCancel" && value.goal !== null) return refuse("Goal still exists after removal");
  const intent = goalIntent(value.goal,threadId);
  if (value.goal !== null && (!intent || !intent.goalStatus)) return refuse("Goal identity or status is invalid");
  const expected = state.goalAction === "pause" ? "paused" : state.goalAction === "resume" ? "active" : state.goalAction === "complete" ? "complete" : undefined;
  if (expected && intent?.goalStatus !== expected) return refuse("Goal status was not confirmed");
  return {kind:"complete",result:{kind:"goal",threadId,intent}};
}

