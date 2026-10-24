import {object,text} from "./runtimeProtocol";
import type { RuntimeErrorSignal } from "@codeterm/plugin-sdk";

function statusCode(value: unknown): number | null {
  if (typeof value === "number" && Number.isInteger(value)) return value;
  if (typeof value === "string" && /^\d+$/.test(value)) return Number(value);
  return null;
}

function nestedStatus(info: Record<string, unknown>): number | null {
  for (const value of Object.values(info)) {
    const code = statusCode(object(value).httpStatusCode);
    if (code != null) return code;
  }
  return null;
}

function runtimeClass(code: string | undefined, status: number | null): string {
  if (code === "token_expired" || code === "refresh_token_expired" || code === "auth_expired" || code === "auth_expiry") return "auth_expiry";
  if (status === 401 || status === 403 || code === "authError" || code === "unauthorized" || code === "authentication_error") return "authentication";
  if (code === "usageLimitExceeded" || code === "sessionBudgetExceeded" || code === "usage_limit") return "usage_limit";
  if (code === "rateLimitExceeded" || code === "rate_limit" || status === 429) return "rate_limit";
  if (code === "serverOverloaded" || code === "internalServerError" || code === "overloaded" || code === "server_overloaded" || (status != null && status >= 500 && status <= 599)) return "server_overloaded";
  if (code?.startsWith("http") || code?.startsWith("responseStream")) return "transport_error";
  return "provider_error";
}

export function runtimeError(value:unknown, willRetry=false, limits?:unknown): {message:string; metadata:RuntimeErrorSignal} {
  const error=object(value);
  const info=error.codexErrorInfo;
  const infoObject=object(info);
  const keys=Object.keys(infoObject);
  const code=typeof info === "string" ? info : keys.length === 1 ? keys[0] : undefined;
  const message=text(error.additionalDetails) ?? text(error.message) ?? "Provider request failed";
  const status=statusCode(error.status) ?? statusCode(error.statusCode) ?? statusCode(error.httpStatusCode) ?? statusCode(infoObject.httpStatusCode) ?? nestedStatus(infoObject);
  return {message,metadata:{...(code ? {code} : {}),class:runtimeClass(code, status),retryable:willRetry,...retrySignal(code,status,limits)}};
}

function retrySignal(code:string|undefined, status:number|null, limits:unknown): Pick<RuntimeErrorSignal,"retryableKind"|"retryAtMs"> {
  if (status === 401 || status === 403) return {};
  const rate = code === "rateLimitExceeded" || code === "usageLimitExceeded";
  const transport = code === "httpConnectionFailed" || code === "responseStreamConnectionFailed" || code === "responseStreamDisconnected" || code === "responseTooManyFailedAttempts";
  const transient = code === "serverOverloaded" || code === "flexUnavailable" || code === "internalServerError";
  if (rate || ((transport || transient) && status === 429)) {
    const snapshot = object(limits);
    if (snapshot.limitId && snapshot.limitId !== "codex") return {};
    const blocked = [snapshot.primary,snapshot.secondary].map(object).filter(window => typeof window.usedPercent === "number" && Number.isFinite(window.usedPercent) && window.usedPercent >= 100);
    if (!blocked.length || blocked.some(window => typeof window.resetsAt !== "number" || !Number.isSafeInteger(window.resetsAt) || window.resetsAt <= 0 || !Number.isSafeInteger(window.resetsAt * 1000))) return {};
    return {retryableKind:"rate_limited_until",retryAtMs:Math.max(...blocked.map(window => Number(window.resetsAt) * 1000))};
  }
  if (transient
    || (transport && (status == null || status === 408 || (status >= 500 && status <= 599)))) return {retryableKind:"transient"};
  return {};
}
