import type {AgentEvent, JsonValue, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {object,text} from "./runtimeProtocol";
import {json} from "./runtimeMessages";

export function loginPlan(raw:JsonValue):RuntimePlan {
  const value=object(raw);
  if (value.action === "cancel" && text(value.loginId)) return {
    action:{kind:"request",call:{method:"account/login/cancel",params:{loginId:String(value.loginId)}}},
    state:{stage:"loginCancel",loginId:String(value.loginId)},
  };
  const types:Record<string,string>={browser:"chatgpt",device:"chatgptDeviceCode",apiKey:"apiKey"};
  const method=String(value.method);
  if (!["browser","device","apiKey"].includes(method) || (method === "apiKey" && !text(value.apiKey))) throw {code:"invalid_schema",message:"Invalid login request"};
  return {action:{kind:"request",call:{method:"account/login/start",params:json({type:types[method],...(method === "apiKey" ? {apiKey:value.apiKey} : {})})}},state:{stage:"loginStart",method}};
}
export function loginResponse(state:Record<string,unknown>,value:Record<string,unknown>):RuntimeStep {
  if (state.stage === "loginCancel") {
    if (value.status !== "canceled" && value.status !== "notFound") throw {code:"invalid_schema",message:"Invalid cancellation result"};
    return {kind:"complete",result:{kind:"done"},events:[{kind:"authLogin",login:{status:"canceled",loginId:String(state.loginId)}}]};
  }
  if (state.method === "apiKey" && value.type === "apiKey") return {kind:"complete",result:{kind:"done"},events:[{kind:"authLogin",login:{status:"completed",method:"apiKey"}}]};
  const loginId=text(value.loginId);
  const url=text(value.authUrl) ?? text(value.verificationUrl);
  if (!loginId || !url || !["chatgpt","chatgptDeviceCode"].includes(String(value.type))) throw {code:"invalid_schema",message:"Invalid login start result"};
  const method=state.method === "device" ? "device" : "browser";
  const userCode=text(value.userCode);
  if (method === "device" && !userCode) throw {code:"invalid_schema",message:"Missing device code"};
  return {kind:"complete",result:{kind:"done"},events:[{kind:"authLogin",login:{status:"pending",method,loginId,url,...(userCode ? {userCode} : {})}}]};
}
// App-server login errors are user-facing reasons; links may carry OAuth state, so they are elided and the text is bounded.
export function loginFailureReason(message:unknown,fallback:string):string {
  const reason=typeof message === "string" ? message.replace(/\bhttps?:\/\/\S+/gi,"<link>").replace(/\s+/g," ").trim() : "";
  if (!reason) return fallback;
  return `${fallback.replace(/\. Try again\.$/,"")}: ${reason.length > 240 ? `${reason.slice(0,239)}…` : reason}`;
}
export function loginCompleted(value:Record<string,unknown>):AgentEvent {
  const loginId=text(value.loginId);
  return {kind:"authLogin",login:{status:value.success === true ? "completed" : "failed",...(loginId ? {loginId} : {}),...(value.success === true ? {} : {error:loginFailureReason(value.error,"Sign-in did not complete. Try again.")})}};
}
