import type {AgentEvent} from "@codeterm/plugin-sdk";
import {object,text} from "./runtimeProtocol";

export function accountAuth(value:unknown): Extract<AgentEvent,{kind:"auth"}> {
  const response=object(value);
  if (response.account === null && response.requiresOpenaiAuth === true) return {kind:"auth",state:"signedOut"};
  const account=object(response.account);
  if (text(account.type)) return {kind:"auth",state:"signedIn",billingMode:account.type === "apiKey" ? "api" : account.type === "chatgpt" || account.type === "chatgptAuthTokens" ? "subscription" : "unknown"};
  return {kind:"auth",state:"unknown"};
}
export function notificationAuth(value:unknown): Extract<AgentEvent,{kind:"auth"}> {
  const params=object(value);
  if (params.authMode === null) return {kind:"auth",state:"signedOut"};
  if (text(params.authMode)) return {kind:"auth",state:"signedIn",billingMode:params.authMode === "apiKey" ? "api" : params.authMode === "chatgpt" || params.authMode === "chatgptAuthTokens" ? "subscription" : "unknown"};
  return {kind:"auth",state:"unknown"};
}
