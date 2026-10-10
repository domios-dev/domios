import {classifyAccessError} from "../../provider-shared/accessError";
import type {ProviderAuthStatus, ProviderSessionObservations} from "@codeterm/plugin-sdk";

export function authStatusProbe() {
  return {args:["auth", "status"], timeoutMs:3000};
}

function object(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
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
  const isError = record.isApiErrorMessage === true;
  const api_error = isError ? classifyAccessError(record.error, record) : null;
  const clears_api_error = record.type === "assistant" && !isError && !!object(record.message);
  return isError || permission_mode || clears_api_error ? {api_error, permission_mode, clears_api_error} : null;
}
