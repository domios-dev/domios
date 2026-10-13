import {object,text} from "./runtimeProtocol";

export function runtimeError(value:unknown, willRetry=false): {message:string; metadata:Record<string,unknown>} {
  const error=object(value);const info=error.codexErrorInfo;
  const code=typeof info === "string" ? info : Object.keys(object(info))[0];
  const message=text(error.additionalDetails) ?? text(error.message) ?? "Provider request failed";
  const category=code === "usageLimitExceeded" || code === "rateLimitExceeded" ? "usage_limit"
    : code?.startsWith("http") || code?.startsWith("responseStream") ? "transport_error" : "provider_error";
  return {message,metadata:{...(code ? {code} : {}),class:category,retryable:willRetry}};
}
