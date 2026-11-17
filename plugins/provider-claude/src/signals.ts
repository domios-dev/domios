import {classifyAccessError, observationExtras} from "../../provider-shared/accessError";
import type {ProviderAuthStatus, ProviderSessionObservations, AccountIdentity} from "@codeterm/plugin-sdk";

import {retryError} from "./retry";

export function authStatusProbe() {
  return {args:["auth", "status"], timeoutMs:15000};
}

function object(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

export function accountIdentity(value: unknown): AccountIdentity | null {
  const record = object(value);
  const email = record?.email;
  if (record?.loggedIn !== true || typeof email !== "string" || !email.includes("@") || email.length > 320 || !/^[\u0021-\u007e]+$/u.test(email)) return null;
  return {subject:email,label:email};
}

export function parseAuthStatus(value: unknown): ProviderAuthStatus | null {
  const record = object(value);
  if (!record || typeof record.loggedIn !== "boolean") return null;
  const result: Record<string, unknown> = {loggedIn:record.loggedIn};
  for (const key of ["authMethod", "subscriptionType", "configDirectory"]) {
    const field = record[key];
    if (field !== undefined && field !== null && (typeof field !== "string" || field.length > 4096)) return null;
    result[key] = field ?? null;
  }
  return result as unknown as ProviderAuthStatus;
}

export function parseSessionObservations(value: unknown): ProviderSessionObservations | null {
  const record = object(value);
  if (!record) return null;
  const modes = ["default", "acceptEdits", "bypassPermissions", "plan", "dontAsk", "auto"];
  const permission_mode = (typeof record.permissionMode === "string" && modes.includes(record.permissionMode) ? record.permissionMode : null) as ProviderSessionObservations["permission_mode"];
  const retry = retryError(record);
  const isError = record.isApiErrorMessage === true || retry.retry_error?.retryableKind === "output_limit";
  const api_error = isError ? classifyAccessError(record.error, record) : null;
  const clears_api_error = record.type === "assistant" && !isError && !!object(record.message);
  return isError || permission_mode || clears_api_error ? {api_error, permission_mode, clears_api_error, ...observationExtras(record.error, record), ...retry, ...(clears_api_error && record.isSidechain !== true && ["end_turn", "stop_sequence"].includes(String(object(record.message)?.stop_reason)) ? {retry_succeeded:true} : {})} : null;
}
