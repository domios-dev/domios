import type {AgentEvent, ChatAttachment, JsonValue, RuntimeContext, RuntimeFailure, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";

export const object = (value:unknown):Record<string,unknown> => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,unknown> : {};
export const list = (value:unknown):unknown[] => Array.isArray(value) ? value : [];
export const text = (value:unknown):string|undefined => typeof value === "string" && value.length > 0 ? value : undefined;
export const json = (value:unknown):JsonValue => value as JsonValue;
export function fail(code:RuntimeFailure["code"], message:string):never { throw {code,message} satisfies RuntimeFailure; }
export function refused(message:string):RuntimeStep { return {kind:"refused",reason:{code:"provider_refused",message},beforeAcceptance:false}; }
export function request(method:string, params:unknown, state:unknown = {stage:method}):RuntimePlan {
  return {action:{kind:"request",call:{method,params:json(params)}},state:json(state)};
}
export function next(method:string, params:unknown, state:unknown):RuntimeStep { return {kind:"continue",plan:request(method,params,state)}; }
export function done(events:readonly AgentEvent[] = []):RuntimeStep { return {kind:"complete",result:{kind:"done"},events}; }
export function own(ctx:RuntimeContext, thread:string|undefined):string {
  if (!thread || thread !== ctx.threadId) fail("binding_mismatch","OpenCode session is outside this runtime binding");
  return thread;
}
export function modelParts(model:string|undefined):{providerID:string;modelID:string}|undefined {
  if (!model) return undefined;
  const index = model.indexOf("/");
  if (index <= 0 || index === model.length - 1) fail("invalid_schema","OpenCode requires a qualified provider/model selection");
  return {providerID:model.slice(0,index),modelID:model.slice(index+1)};
}
export function messageIdentity(sendId:string, now:number, counter:number):string {
  if (!/^[a-zA-Z0-9_-]{1,128}$/.test(sendId) || !Number.isSafeInteger(now) || now < 0) fail("invalid_schema","Invalid prompt identity");
  const prefix = ((BigInt(now) * 0x1000n + BigInt(counter)) & 0xffffffffffffn).toString(16).padStart(12,"0");
  return `msg_${prefix}${sendId.replace(/-/g,"_")}`;
}
export function attachmentUrl(attachment:ChatAttachment):string {
  if (attachment.data) return `data:${attachment.mimeType};base64,${attachment.data}`;
  if (attachment.url) return attachment.url;
  if (!attachment.path) fail("invalid_schema","Attachment has no runtime source");
  const normalized = attachment.path.replace(/\\/g,"/");
  return `file://${normalized.startsWith("/") ? "" : "/"}${normalized.split("/").map(segment => encodeURIComponent(segment).replace(/%3A/gi,":")).join("/")}`;
}
export function utf8Length(value:string):number {
  let bytes=0;
  for (const character of value) {const code=character.codePointAt(0)!;bytes+=code <= 127 ? 1 : code <= 2047 ? 2 : code <= 65535 ? 3 : 4;}
  return bytes;
}
