import type {ProviderSessionObservations} from "@codeterm/plugin-sdk";
import {classifyAccessError} from "../../provider-shared/accessError";

export function parseSessionObservations(value: unknown): ProviderSessionObservations | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (record.type !== "event_msg" || !record.payload || typeof record.payload !== "object" || Array.isArray(record.payload)) return null;
  const payload = record.payload as Record<string, unknown>;
  if (payload.type === "error") return {
    api_error: classifyAccessError(payload.error ?? payload, payload), permission_mode: null, clears_api_error: false,
  };
  if (payload.type === "task_complete") return {api_error:null, permission_mode:null, clears_api_error:true};
  return null;
}
