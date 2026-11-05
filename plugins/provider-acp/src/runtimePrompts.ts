import type { JsonValue, PendingPrompt, PromptAnswer, RpcId, RuntimeContext, RuntimeFailure } from "@codeterm/plugin-sdk";

const kinds = new Set(["allow_once", "allow_always", "reject_once", "reject_always"]);
const object = (value:unknown):Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
function fail(message:string):never { throw {code:"provider_refused",message} satisfies RuntimeFailure; }
export function runtimePrompt(ctx:RuntimeContext, method:string, params:JsonValue):PendingPrompt|undefined {
  const data=object(params);
  if (method !== "session/request_permission" || !ctx.threadId || data.sessionId !== ctx.threadId) return undefined;
  const tool=object(data.toolCall);
  if (typeof tool.toolCallId !== "string" || !tool.toolCallId || !Array.isArray(data.options) || !data.options.length || data.options.length > 16) return undefined;
  const seen=new Set<string>();
  const options=data.options.map(object);
  if (options.some(option=>{
    if (typeof option.optionId !== "string" || !option.optionId || option.optionId.length > 4096
      || seen.has(option.optionId) || typeof option.name !== "string" || option.name.length > 4096
      || typeof option.kind !== "string" || !kinds.has(option.kind)) return true;
    seen.add(option.optionId);
    return false;
  })) return undefined;
  const question=JSON.stringify({toolCall:tool});
  if (question.length > 65536) return undefined;
  return {question,options:options.map(option=>({label:`${option.name} (${option.kind})`,value:String(option.optionId)}))};
}
export function runtimeAnswer(ctx:RuntimeContext, method:string, params:JsonValue, _id:RpcId, answer:PromptAnswer):JsonValue {
  const prompt=runtimePrompt(ctx,method,params);
  if (!prompt) fail("Permission request does not belong to this session");
  if (answer.kind === "cancel") return {outcome:{outcome:"cancelled"}};
  if (answer.kind !== "choice" || answer.values.length !== 1 || !prompt.options.some(option=>option.value === answer.values[0]))
    fail("Permission requires one declared optionId");
  return {outcome:{outcome:"selected",optionId:answer.values[0]}};
}
