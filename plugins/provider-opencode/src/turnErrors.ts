import type { ProviderSessionObservations } from "@codeterm/plugin-sdk";

interface Message {
  role?: string;
  error?: { name?: string; data?: { message?: string; statusCode?: number; isRetryable?: boolean } };
}

export function turnError(value: unknown): Message["error"] | null {
  if (!value || typeof value !== "object") return null;
  const message = value as Message;
  return message.role === "assistant" && message.error?.name === "APIError" ? message.error : null;
}

export function sessionObservations(value: unknown): ProviderSessionObservations | null {
  const error = turnError(value);
  if (!error) {
    const message = value as { role?: string; error?: unknown; time?: { completed?: number } } | null;
    return message?.role === "assistant" && !message.error && typeof message.time?.completed === "number"
      ? { api_error: null, permission_mode: null, clears_api_error: true } : null;
  }
  const status = error.data?.statusCode;
  const message = value as { providerID?: string; modelID?: string };
  const id = message.modelID?.includes("/") ? message.modelID : message.providerID && message.modelID ? `${message.providerID}/${message.modelID}` : null;
  return { ...(id && unavailableModelError(value) ? { unavailable_model: id } : {}), api_error: status === 401 || status === 403 ? "authentication" : status === 429 ? "rate_limit" : "other", permission_mode: null, clears_api_error: false };
}

export function unavailableModelError(value: unknown): boolean {
  const error = turnError(value);
  return error?.data?.isRetryable === false && (error.data.statusCode === 410
    || /\bmodel\b.*\b(deprecated|unavailable|not found|does not exist|not supported)\b/i.test(error.data.message ?? ""));
}

export function modelMetadata(stdout: string): Map<string, Record<string, unknown>> {
  const metadata = new Map<string, Record<string, unknown>>();
  let id: string | null = null;
  let lines: string[] = [];
  const finish = () => {
    if (!id) return;
    try {
      const value: unknown = JSON.parse(lines.join("\n"));
      if (value && typeof value === "object" && !Array.isArray(value)) metadata.set(id, value as Record<string, unknown>);
    } catch { }
  };
  for (const line of stdout.split("\n")) {
    if (/^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(line.trim())) {
      finish(); id = line.trim(); lines = [];
    } else if (id) lines.push(line);
  }
  finish();
  return metadata;
}

export function modelUnavailable(metadata?: Record<string, unknown>): boolean {
  return metadata?.status === "deprecated" || metadata?.status === "unavailable"
    || metadata?.deprecated === true || metadata?.available === false;
}

export function rejectedModelIds(rows: Array<{ model: string | null; message: unknown }>): Set<string> {
  const seen = new Set<string>();
  const rejected = new Set<string>();
  for (const { model, message } of rows) {
    if (!model || seen.has(model)) continue;
    seen.add(model);
    if (unavailableModelError(message)) rejected.add(model);
  }
  return rejected;
}
