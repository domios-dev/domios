import {object,text} from "./runtimeProtocol";

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

export function runtimeError(value:unknown, willRetry=false): {message:string; metadata:Record<string,unknown>} {
  const error=object(value);
  const info=error.codexErrorInfo;
  const infoObject=object(info);
  const code=typeof info === "string" ? info : Object.keys(infoObject)[0];
  const message=text(error.additionalDetails) ?? text(error.message) ?? "Provider request failed";
  const status=statusCode(error.status) ?? statusCode(error.statusCode) ?? statusCode(error.httpStatusCode) ?? statusCode(infoObject.httpStatusCode) ?? nestedStatus(infoObject);
  return {message,metadata:{...(code ? {code} : {}),class:runtimeClass(code, status),retryable:willRetry}};
}
