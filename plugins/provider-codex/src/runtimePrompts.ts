import { canonicalJson } from "./canonicalJson";
import type { PendingPrompt, PromptAnswer, RpcId, JsonValue, RuntimeFailure } from "@codeterm/plugin-sdk";
import { object, list, text } from "./runtimeProtocol";
import { json } from "./runtimeMessages";
import { nativeOption } from "./runtimeQuestions";

function fail(message: string): never {
  throw {code:"provider_refused",message} satisfies RuntimeFailure;
}
function decisionOptions(data:Record<string,unknown>): {label:string;value:string}[] {
  const decisions=list(data.availableDecisions);
  const supported=decisions.length ? [...decisions] : ["accept","acceptForSession","decline","cancel"];
  if (!supported.includes("decline")) supported.push("decline");
  const labels:Record<string,string>={accept:"Approve",acceptForSession:"Approve for session",decline:"Decline",cancel:"Cancel"};
  return supported.map(value=>({label:typeof value === "string" ? labels[value] ?? value : "Approve with the proposed rule",value:typeof value === "string" ? value : canonicalJson(value)}));
}
export function runtimePrompt(method: string, params: unknown, _id: RpcId): PendingPrompt | undefined {
  const data = object(params);
  if (method === "item/tool/requestUserInput") {
    const questions = list(data.questions).map(object);
    if (!questions.length) return undefined;
    return {
      question:questions.map(q => String(q.question ?? "")).join("\n"),
      options:questions.flatMap(q => list(q.options).flatMap(o=>{const parsed=nativeOption(o);return parsed ? [parsed] : [];})),
      sections:questions.map(q => ({label:String(q.header ?? q.question ?? "")})),
      multi_select:questions.length > 1,
    };
  }
  switch (method) {
    case "item/commandExecution/requestApproval": case "item/fileChange/requestApproval":
      return {question:String(data.command ?? data.reason ?? "Approve this operation?"),options:decisionOptions(data)};
    case "item/permissions/requestApproval":
      return {question:`Grant permissions: ${canonicalJson(data.permissions)}`,options:[{label:"Grant for turn",value:"turn"},{label:"Grant for session",value:"session"},{label:"Deny",value:"deny"}]};
    case "mcpServer/elicitation/request":
      if (data.mode !== "form" || list(object(data.requestedSchema).required).length)
        return {question:String(data.message ?? "Unsupported tool request"),options:[{label:"Decline",value:"decline"},{label:"Cancel",value:"cancel"}]};
      return {question:String(data.message ?? "Respond to the tool request"),options:[{label:"Accept",value:"accept"},{label:"Decline",value:"decline"},{label:"Cancel",value:"cancel"}]};
    default: return undefined;
  }
}
export function runtimeAnswer(method: string, params: unknown, id: RpcId, answer: PromptAnswer): JsonValue {
  const data = object(params);
  const values = answer.kind === "choice" ? answer.values : [];
  const prompt = runtimePrompt(method,params,id);
  if (!prompt) fail("Unsupported server request");
  if (values.some(value => !prompt.options.some(o => o.value === value))) fail("Invalid native option value");
  if (method === "item/tool/requestUserInput") {
    const questions = list(data.questions).map(object);
    const answers: Record<string, unknown> = {};
    if (answer.kind === "cancel") fail("Questions cannot be cancelled without an answer");
    for (const q of questions) {
      const key = text(q.id);
      if (!key) fail("Missing question id");
      const selected = list(q.options).flatMap(o=>{const parsed=nativeOption(o);return parsed && values.includes(parsed.value) ? [parsed.value] : [];});
      const value = answer.kind === "text" && questions.length === 1 ? answer.text : selected.length === 1 ? selected[0] : undefined;
      if (!value?.trim()) fail("Each question requires one answer");
      answers[key] = {answers:[value]};
    }
    return json({answers});
  }
  if (answer.kind === "text" || (answer.kind === "choice" && values.length !== 1)) fail("Approval requires one declared option");
  const value = answer.kind === "cancel" ? "cancel" : values[0];
  switch (method) {
    case "item/commandExecution/requestApproval": case "item/fileChange/requestApproval":
      return {decision:value.startsWith("{") ? JSON.parse(value) : value};
    case "item/permissions/requestApproval":
      return json({permissions:value === "turn" || value === "session" ? data.permissions ?? {} : {},scope:value === "session" ? "session" : "turn"});
    case "mcpServer/elicitation/request":
      return json({action:value,...(value === "accept" ? {content:{}} : {})});
    default: return fail("Unsupported server request");
  }
}
