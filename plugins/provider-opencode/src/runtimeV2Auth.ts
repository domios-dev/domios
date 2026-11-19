import type {AgentEvent, Operation, RuntimeAuthentication, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {done, fail, list, modelParts, next, object, refused, request, text} from "./runtimeProtocol";
import {binding} from "./runtimeState";

type Control=Extract<Operation,{kind:"control"}>;
function selected(ctx:RuntimeContext):string {
  const provider=modelParts(ctx.settings.model)?.providerID ?? "";
  return binding(ctx).providers?.find(entry=>entry.id === provider)?.integration ?? provider;
}
export function v2Providers(ctx:RuntimeContext, body:unknown):void {
  const entries=list(object(body).data);
  if (entries.length > 512) fail("size_limit","OpenCode provider catalogue exceeds its bound");
  binding(ctx).providers=entries.map(raw=>{const item=object(raw), id=text(item.id);if (!id) fail("invalid_schema","Provider has no identity");return {id,integration:text(item.integrationID) ?? id};});
}
function integration(ctx:RuntimeContext, body:unknown):Record<string,unknown>|undefined {
  const entries=list(object(body).data);
  if (entries.length > 1024) fail("size_limit","OpenCode integration catalogue exceeds its bound");
  return entries.map(object).find(entry=>entry.id === selected(ctx));
}
function methods(ctx:RuntimeContext, body:unknown):RuntimeAuthentication["methods"] {
  const entry=integration(ctx,body);
  if (!entry) return [];
  const entries=list(entry.methods);
  if (entries.length > 32) fail("size_limit","OpenCode authentication methods exceed their bound");
  return entries.filter(raw=>["key","oauth"].includes(String(object(raw).type))).map(raw=>{
    const method=object(raw), oauth=method.type === "oauth", id=oauth ? text(method.id) : "key";
    if (!id || id.length > 256) fail("invalid_schema","OpenCode auth method has no bounded identity");
    const unavailable=list(method.form).some(raw=>{const field=object(raw);return field.hidden !== true || field.default === undefined;});
    return {id:`v2:${encodeURIComponent(String(entry.id))}:${encodeURIComponent(id)}`,name:`${String(entry.name)}: ${String(method.label ?? (oauth ? id : "API key"))}`,input:oauth ? "none" : "apiKey",available:!unavailable,...(unavailable ? {unavailableReason:"This sign-in method requires provider-specific form fields"} : {})};
  });
}
function connected(entry:Record<string,unknown>|undefined):boolean {
  return !!entry && list(entry.connections).some(raw=>object(object(raw).status).status !== "needs_auth");
}
export function v2AuthEvents(ctx:RuntimeContext, body:unknown):AgentEvent[] {
  const entry=integration(ctx,body), discovered=methods(ctx,body);
  const needsAuth=!!entry && list(entry.connections).length > 0 && !connected(entry);
  const available=!needsAuth && !!binding(ctx).providers?.some(entry=>entry.id === modelParts(ctx.settings.model)?.providerID);
  binding(ctx).methods=discovered;
  if (binding(ctx).login?.observed) delete binding(ctx).login;
  return [{kind:"authentication",authentication:{state:available || connected(entry) ? "authenticated" : entry ? "needs_auth" : "unknown",methods:discovered}},
    {kind:"auth",state:available || connected(entry) ? "signedIn" : entry ? "signedOut" : "unknown"}];
}
export function v2LoginEvents(ctx:RuntimeContext, frames:readonly RuntimeFrame[], nowMs=Date.now()):AgentEvent[] {
  const pending=binding(ctx).login;
  if (!pending || pending.observed || pending.integration !== selected(ctx) || nowMs >= pending.expires) return [];
  for (const frame of frames) {
    const event=object(frame.params), data=object(event.data), created=event.created;
    if (frame.kind !== "notification" || frame.method !== "http/event" || event.type !== "credential.updated" || !text(event.id) || !text(data.credentialID) || data.integrationID !== pending.integration) continue;
    if (typeof created !== "number" || !Number.isSafeInteger(created) || created <= pending.created || created > nowMs || created >= pending.expires) continue;
    const directory=text(object(event.location).directory);
    if (directory && directory.replace(/\\/g,"/").replace(/\/$/,"") !== ctx.cwd.replace(/\\/g,"/").replace(/\/$/,"")) continue;
    pending.observed=true;
    return [{kind:"authLogin",login:{status:"completed",loginId:pending.loginId}}];
  }
  return [];
}
function identity(ctx:RuntimeContext, op:Control):{loginId:string;methodId:string;integration:string;method:string;value:Record<string,unknown>} {
  const value=object(op.value), loginId=text(value.loginId), methodId=text(value.methodId), parts=methodId?.split(":");
  if (!loginId || loginId.length > 128 || parts?.length !== 3 || parts[0] !== "v2") fail("binding_mismatch","Sign-in requires its exact host challenge");
  let provider:string, method:string;
  try {provider=decodeURIComponent(parts[1]);method=decodeURIComponent(parts[2]);} catch {fail("invalid_schema","Invalid sign-in method identity");}
  if (provider !== selected(ctx) || !method) fail("binding_mismatch","Sign-in targets another selected provider");
  return {loginId,methodId:methodId!,integration:provider,method,value};
}
function query(ctx:RuntimeContext):Record<string,string> {return {"location[directory]":ctx.cwd};}
export function v2LoginPlan(ctx:RuntimeContext, op:Control):RuntimePlan {
  const id=identity(ctx,op);
  if (id.value.action === "start") return request("v2.integration.list",{query:query(ctx)},{stage:"loginMethods"});
  const pending=binding(ctx).login;
  if (!pending || pending.loginId !== id.loginId || pending.methodId !== id.methodId) fail("binding_mismatch","Sign-in callback has no matching OAuth attempt");
  const path={integrationID:pending.integration,attemptID:pending.attempt};
  if (id.value.action === "cancel") return request("v2.integration.cancel",{path,query:query(ctx)},{stage:"loginCancel"});
  if (id.value.action === "complete") return request("v2.integration.status",{path,query:query(ctx)},{stage:"loginStatus"});
  if (id.value.action !== "code" || !text(id.value.code) || String(id.value.code).length > 16384) fail("invalid_schema","A bounded sign-in code is required");
  return request("v2.integration.complete",{path,query:query(ctx),body:{code:id.value.code}},{stage:"loginCode"});
}
export function v2LoginResponse(ctx:RuntimeContext, op:Control, state:Record<string,unknown>, body:unknown):RuntimeStep {
  const id=identity(ctx,op), data=object(object(body).data);
  if (binding(ctx).login?.observed && ["loginStatus","loginCode","loginVerify"].includes(String(state.stage))) return done();
  const event=(status:"completed"|"failed",error?:string)=>done([{kind:"authLogin",login:{status,loginId:id.loginId,...(error ? {error} : {})}}]);
  switch(state.stage) {
    case "loginMethods": {
      if (!methods(ctx,body).some(method=>method.id === id.methodId && method.available)) return refused("The sign-in method is no longer available");
      const path={integrationID:id.integration};
      if (id.method !== "key") {
        const method=list(integration(ctx,body)?.methods).map(object).find(method=>method.id === id.method);
        const answer=Object.fromEntries(list(method?.form).map(object).map(field=>[String(field.key),field.default]));
        return next("v2.integration.authorize",{path,query:query(ctx),body:{methodID:id.method,...(Object.keys(answer).length ? {answer} : {})}},{stage:"loginAuthorize"});
      }
      const key=text(id.value.apiKey);
      if (!key || key.length > 16384) return refused("A bounded API key is required");
      return next("v2.integration.key",{path,query:query(ctx),body:{key}},{stage:"loginKey"});
    }
    case "loginAuthorize": {
      const attempt=text(data.attemptID), url=text(data.url), instructions=text(data.instructions), expiry=object(data.time).expires, created=object(data.time).created;
      if (!attempt || attempt.length > 256 || !url || !/^https?:\/\//.test(url) || url.length > 4096 || !["auto","code"].includes(String(data.mode)) || instructions && instructions.length > 4096) return refused("Unsupported OpenCode OAuth challenge");
      if (typeof created !== "number" || !Number.isSafeInteger(created) || typeof expiry !== "number" || !Number.isSafeInteger(expiry) || expiry <= created) return refused("OpenCode OAuth challenge has no bounded lifetime");
      const challenge={status:"pending" as const,method:"browser" as const,loginId:id.loginId,url,codeInput:data.mode === "code" || data.paste === true,...(instructions ? {instructions} : {}),expiresAtMs:expiry};
      binding(ctx).login={loginId:id.loginId,methodId:id.methodId,integration:id.integration,attempt,created,expires:expiry,challenge};
      return done([{kind:"authLogin",login:challenge}]);
    }
    case "loginKey":
      if (body !== null) return event("failed","OpenCode did not acknowledge authentication");
      return next("v2.integration.list",{query:query(ctx)},{stage:"loginVerify"});
    case "loginCode": {
      if (body !== null) return event("failed","OpenCode did not acknowledge the sign-in code");
      const pending=binding(ctx).login;
      if (!pending) return refused("Sign-in code has no active OAuth attempt");
      return next("v2.integration.status",{path:{integrationID:pending.integration,attemptID:pending.attempt},query:query(ctx)},{stage:"loginStatus"});
    }
    case "loginStatus":
      if (data.status === "pending") {
        const pending=binding(ctx).login;
        return pending ? done([{kind:"authLogin",login:pending.challenge}]) : refused("Sign-in status has no active OAuth attempt");
      }
      if (data.status !== "complete") {delete binding(ctx).login;return event("failed","OpenCode sign-in did not complete");}
      return next("v2.integration.list",{query:query(ctx)},{stage:"loginVerify"});
    case "loginVerify": {
      const success=connected(integration(ctx,body));
      delete binding(ctx).login;
      return event(success ? "completed" : "failed",success ? undefined : "OpenCode did not confirm a connected account");
    }
    case "loginCancel":delete binding(ctx).login;return event("failed","Sign-in canceled");
    default:return refused("Unknown OpenCode sign-in response");
  }
}
