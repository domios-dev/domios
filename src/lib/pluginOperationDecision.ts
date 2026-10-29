import { intentGesture, viewHostResult, type ViewHostGate, type ViewHostIntent, type ViewHostResult } from "./pluginViewHostActions";

export interface PluginOperationDecision {
  operation: string;
  decisionKey: string;
  action: "approve" | "check_price" | "check" | "cancel";
}

export interface OperationDecisionHost {
  send(decision: PluginOperationDecision | (Omit<PluginOperationDecision, "action"> & {action:"preview"})): Promise<unknown>;
  confirm(quote: OperationQuote): boolean | Promise<boolean>;
  settled?(result: ViewHostResult): void;
}

export interface OperationQuote { amount:number; unit:string; amountDisplay:string; runLabel:string; nodeLabel:string; aboveRunBudget:boolean }

function confirmation(value: unknown, decision: PluginOperationDecision): OperationQuote | null {
  if (!value || typeof value !== "object" || (value as {ok?:unknown}).ok !== true) return null;
  const preview = (value as {preview?:unknown}).preview;
  if (!preview || typeof preview !== "object") return null;
  const quote = preview as Record<string, unknown>;
  if (quote.operation !== decision.operation || quote.decisionKey !== decision.decisionKey
    || !Number.isSafeInteger(quote.amount) || Number(quote.amount) < 0
    || typeof quote.unit !== "string" || !/^[a-z][a-z0-9_-]{0,63}$/.test(quote.unit)
    || typeof quote.amountDisplay !== "string" || quote.amountDisplay.length > 128 || !/^[A-Za-z0-9 ()._-]+$/.test(quote.amountDisplay)
    || ![quote.runLabel, quote.nodeLabel].every(label => typeof label === "string" && label.length > 0 && label.length <= 128 && !/[\u0000-\u001f\u007f]/.test(label))) return null;
  return {amount:Number(quote.amount),unit:quote.unit,amountDisplay:quote.amountDisplay,runLabel:String(quote.runLabel),nodeLabel:String(quote.nodeLabel),aboveRunBudget:quote.aboveRunBudget === true};
}

export async function decidePluginOperation(value: unknown, intent: ViewHostIntent, isCurrent: () => boolean,
  gate: ViewHostGate, host: OperationDecisionHost): Promise<ViewHostResult> {
  const decision = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!decision || typeof decision.operation !== "string" || !/^[a-f0-9]{64}$/.test(decision.operation)
    || typeof decision.decisionKey !== "string" || !new RegExp(`^blueprint-plugin:${decision.operation}:[a-f0-9]{64}$`).test(decision.decisionKey)
    || !["approve","check_price","check","cancel"].includes(String(decision.action))
    || Object.keys(decision).some((key) => !["operation","decisionKey","action"].includes(key))) {
    return {ok:false,code:"invalid",reason:"decision"};
  }
  if (!intentGesture(intent)) return {ok:false,code:"denied",reason:"gesture"};
  if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
  if (!gate.claim()) return {ok:false,code:"denied",reason:"busy"};
  try {
    const request = decision as unknown as PluginOperationDecision;
    if (request.action === "approve") {
      const preview = await host.send({...request,action:"preview"});
      if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
      const message = confirmation(preview, request);
      if (!message) return {ok:false,code:"denied",reason:"quote"};
      if (!await host.confirm(message)) return {ok:false,code:"denied",reason:"cancelled"};
      if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
    }
    const result = await host.send(request);
    if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
    const raw = viewHostResult(result);
    const reply: ViewHostResult = request.action === "approve" && !raw.ok && raw.code === "denied" ? {ok:false,code:"denied",reason:"stale"} : raw;
    host.settled?.(reply);
    return reply.ok ? {ok:true} : reply;
  } catch {
    const result: ViewHostResult = isCurrent() ? {ok:false,code:"unavailable",reason:"operation"} : {ok:false,code:"denied",reason:"stale"};
    host.settled?.(result);
    return result;
  }
  finally { gate.release(true); }
}
