import type {AgentEvent, Operation, RuntimeContext, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {done, fail, list, modelParts, next, object, refused, request, text} from "./runtimeProtocol";

type Control = Extract<Operation,{kind:"control"}>;
export const providerListParams = {responseFields:{all:["id","name"],connected:[]}};
interface Method {id:string;name:string;available:boolean;input:"none"|"apiKey";unavailableReason?:string}
interface Selection {provider:string;index?:number;label?:string;kind:"api"|"oauth"}

export function methods(ctx:RuntimeContext, providers:unknown, auth:unknown):Method[] {
  const provider = modelParts(ctx.settings.model)?.providerID;
  if (!provider) return [];
  const declared = list(object(providers).all).map(object).find(item => item.id === provider);
  if (!declared) return [];
  const name = text(declared.name) ?? provider;
  const entries = list(object(auth)[provider]);
  if (entries.length > 31) fail("size_limit","OpenCode auth method capacity exceeded");
  const result:Method[] = [];
  entries.forEach((raw,index) => {
    const method = object(raw);
    if (method.type !== "oauth" && method.type !== "api") {
      result.push({id:`unsupported:${encodeURIComponent(provider)}:${index}`,name:`${name}: unsupported sign-in method`,available:false,input:"none",unavailableReason:"The server declared an unsupported authentication method"});
      return;
    }
    const label = text(method.label) ?? `Method ${index+1}`;
    const extra = list(method.prompts).length > 0;
    result.push({id:`${method.type}:${encodeURIComponent(provider)}:${index}:${encodeURIComponent(label)}`,name:`${name}: ${label}`,available:!extra,input:method.type === "api" ? "apiKey" : "none",...(extra ? {unavailableReason:"This sign-in method requires unsupported provider-specific fields"} : {})});
  });
  result.push({id:`api:${encodeURIComponent(provider)}`,name:`${name}: API key`,available:true,input:"apiKey"});
  return result;
}

export function authEvents(ctx:RuntimeContext, providers:unknown, auth:unknown):AgentEvent[] {
  const provider = modelParts(ctx.settings.model)?.providerID;
  const known = !!provider && list(object(providers).all).map(object).some(item => item.id === provider);
  const connected = !!provider && list(object(providers).connected).includes(provider);
  return [{kind:"authentication",authentication:{state:!known ? "unknown" : connected ? "authenticated" : "needs_auth",methods:methods(ctx,providers,auth)}},
    {kind:"auth",state:!known ? "unknown" : connected ? "signedIn" : "signedOut"}];
}

function select(id:string):Selection {
  const parts = id.split(":");
  let provider:string, label:string|undefined;
  try { provider = decodeURIComponent(parts[1] ?? ""); label = parts[3] ? decodeURIComponent(parts[3]) : undefined; }
  catch { fail("invalid_schema","Invalid sign-in method identity"); }
  if (!provider || !["api","oauth"].includes(parts[0]) || !(parts.length === 2 && parts[0] === "api" || parts.length === 4 && /^\d+$/.test(parts[2]))) fail("invalid_schema","Invalid sign-in method identity");
  return {provider,kind:parts[0] as "api"|"oauth",...(parts.length === 4 ? {index:Number(parts[2]),label} : {})};
}
function loginValue(op:Control):{id:string;loginId:string;selection:Selection;value:Record<string,unknown>} {
  const value = object(op.value), id = text(value.methodId), loginId = text(value.loginId);
  if (!id || !loginId || loginId.length > 128) fail("binding_mismatch","Sign-in requires its host-owned challenge identity");
  return {id,loginId,selection:select(id),value};
}
function loginEvent(loginId:string, status:"completed"|"failed", error?:string):AgentEvent {
  return {kind:"authLogin",login:{status,loginId,...(error ? {error} : {})}};
}
export function loginPlan(ctx:RuntimeContext, op:Control):RuntimePlan {
  const {selection,value} = loginValue(op);
  if (modelParts(ctx.settings.model)?.providerID !== selection.provider) fail("binding_mismatch","Sign-in method targets another selected provider");
  if (value.action === "start") return request("provider.list",providerListParams, {stage:"loginProviders"});
  if (value.action !== "complete" && value.action !== "code") fail("unsupported","Unsupported sign-in challenge action");
  if (selection.kind !== "oauth" || selection.index === undefined) fail("invalid_schema","Sign-in callback requires its discovered OAuth method");
  const code = value.action === "code" ? text(value.code) : undefined;
  if (value.action === "code" && (!code || code.length > 16384)) fail("invalid_schema","A bounded sign-in code is required");
  return request("provider.callback",{path:{providerID:selection.provider},body:{method:selection.index,...(code ? {code} : {})}},{stage:"loginCallback"});
}
export function loginResponse(ctx:RuntimeContext, op:Control, state:Record<string,unknown>, body:unknown):RuntimeStep {
  const {id,loginId,selection,value} = loginValue(op);
  switch (state.stage) {
    case "loginProviders": return next("provider.auth",{}, {stage:"loginMethods",providers:providerSummary(body)});
    case "loginMethods": {
      const chosen = methods(ctx,state.providers,body).find(method => method.id === id && method.available);
      if (!chosen) return refused("The selected sign-in method is no longer available");
      if (selection.kind === "oauth") return next("provider.authorize",{path:{providerID:selection.provider},body:{method:selection.index}},{stage:"loginAuthorize"});
      const key = text(value.apiKey);
      if (!key || key.length > 16384) return refused("A bounded API key is required");
      return next("provider.key",{path:{providerID:selection.provider},body:{type:"api",key}},{stage:"loginKey"});
    }
    case "loginAuthorize": {
      const challenge = object(body), url = text(challenge.url), method = challenge.method;
      if (!url || !/^https?:\/\//.test(url) || url.length > 4096 || (method !== "auto" && method !== "code")) return refused("The server returned an unsupported sign-in challenge");
      const instructions = text(challenge.instructions);
      if (instructions && instructions.length > 4096) return refused("The sign-in instructions exceeded their display budget");
      const login = {status:"pending" as const,method:"browser" as const,loginId,url,codeInput:method === "code",...(instructions ? {instructions} : {})};
      return done([{kind:"authLogin",login}]);
    }
    case "loginKey": case "loginCallback": {
      if (body !== true) return done([loginEvent(loginId,"failed","The server did not confirm authentication")]);
      return next("provider.list",providerListParams, {stage:"loginVerify"});
    }
    case "loginVerify": return done([loginEvent(loginId,list(object(body).connected).includes(selection.provider) ? "completed" : "failed",list(object(body).connected).includes(selection.provider) ? undefined : "The provider did not confirm a connected account")]);
    default: return refused("Unknown sign-in response stage");
  }
}
export function providerSummary(value:unknown):unknown {
  const all=list(object(value).all), connected=list(object(value).connected);
  if (all.length > 512 || connected.length > 512) fail("size_limit","OpenCode provider discovery capacity exceeded");
  return {all:all.map(raw => {
    const provider=object(raw), id=text(provider.id), name=text(provider.name) ?? id;
    if (!id || id.length > 256 || !name || name.length > 256) fail("invalid_schema","Invalid provider discovery identity");
    return {id,name};
  }),connected};
}
