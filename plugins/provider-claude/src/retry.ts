import type {RuntimeErrorSignal} from "@codeterm/plugin-sdk";
import {classifyAccessError, observationRetrying, retryAfterSecs} from "../../provider-shared/accessError";

const networkCodes = ["ENOTFOUND", "ECONNRESET", "ECONNREFUSED", "ETIMEDOUT", "EAI_AGAIN", "EPIPE", "UND_ERR_CONNECT_TIMEOUT", "api_connection_error", "connection_error"];

export function retryError(record: Record<string, unknown>): {retry_error?: RuntimeErrorSignal} {
  if (record.type !== "assistant" || record.isApiErrorMessage !== true || record.isSidechain === true) return {};
  const kind = classifyAccessError(record.error, record);
  const error = record.error && typeof record.error === "object" ? record.error as Record<string, unknown> : {};
  const message = record.message as {content?: {type?: string; text?: string}[]} | undefined;
  const fields = [record.error, record.apiErrorCode, error.type, error.code,
    ...(Array.isArray(message?.content) ? message.content.filter(p => p != null && p.type === "text").map(p => p.text) : [])];
  const network = fields.some(field => typeof field === "string" && field.length <= 16384 && field.split(/[^A-Za-z0-9_]+/).some(token => networkCodes.includes(token)));
  let retryableKind: RuntimeErrorSignal["retryableKind"];
  let retryAtMs: number | undefined;
  if (kind === "rate_limit") {
    const reset = record.apiErrorResetAtMs ?? error.retryAtMs;
    const seconds = retryAfterSecs(record.error, record);
    const timestamp = typeof record.timestamp === "string" ? Date.parse(record.timestamp) : NaN;
    const due = reset ?? (seconds != null ? timestamp + seconds * 1000 : NaN);
    if (typeof due !== "number" || !Number.isSafeInteger(due) || due < 0) return {};
    retryableKind = "rate_limited_until";
    retryAtMs = due;
  } else if (kind === "server_overloaded" || (kind === "other" && network)) {
    retryableKind = "transient";
  } else return {};
  return {retry_error:{retryable:observationRetrying(record.error, record), retryableKind, ...(retryAtMs === undefined ? {} : {retryAtMs})}};
}
