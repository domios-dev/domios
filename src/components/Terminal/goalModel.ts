import type { ChatSessionIntent } from "@codeterm/chat-core";
import type { ProviderDescriptor } from "../../lib/types";
export function goalVisible(capability: ProviderDescriptor["agent_runtime"], intent: ChatSessionIntent | null | undefined): boolean {
  return capability?.goals !== undefined && Boolean(intent?.goal?.trim());
}
export type GoalAction = "cancel" | "pause" | "resume" | "complete";
export interface GoalBinding {threadId:string; bindingNonce:string; epoch:number}
export type GoalSnapshot = {supported:false} | {supported:true; binding:GoalBinding; intent:ChatSessionIntent|null; actions:readonly string[]};
export const goalActionLabels:Record<GoalAction,string> = {cancel:"Cancel goal",pause:"Pause goal",resume:"Resume goal",complete:"Complete goal"};
export function goalActions(snapshot:GoalSnapshot|null):GoalAction[] {
  if (!snapshot?.supported || !snapshot.intent?.goal) return [];
  const status = snapshot.intent.goalStatus;
  return (["pause","resume","complete","cancel"] as const).filter(action => snapshot.actions.includes(action)
    && (action !== "pause" || status === "active")
    && (action !== "resume" || status !== "active" && status !== "complete")
    && (action !== "complete" || status !== "complete"));
}
export function goalStatusLabel(status?:string):string {
  return status === "usageLimited" ? "Usage limited" : status === "budgetLimited" ? "Budget limited" : status ? status[0].toUpperCase()+status.slice(1) : "Unknown";
}
export function goalMutation(snapshot:GoalSnapshot,action:GoalAction) {
  if (!snapshot.supported || !goalActions(snapshot).includes(action)) throw new Error("Goal action unavailable");
  return {action,confirmed:true,binding:snapshot.binding,expectedIntent:snapshot.intent};
}
export function goalConfirmation(action:GoalAction):string {
  return action === "cancel" ? "Remove this goal from the provider session?" : action === "complete" ? "Mark this goal as achieved?" : action === "pause" ? "Pause work toward this goal?" : "Resume work toward this goal?";
}
