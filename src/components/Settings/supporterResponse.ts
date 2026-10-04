import type { SupporterAdapter, SupporterStatus } from "./supporterTypes";

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function isSupporterCatalog(value: unknown): value is { adapters: SupporterAdapter[] } {
  return (
    object(value) &&
    Array.isArray(value.adapters) &&
    value.adapters.every(
      (adapter: unknown) =>
        object(adapter) &&
        typeof adapter.id === "string" &&
        ["required", "optional", "none"].includes(String(adapter.token_mode)) &&
        Array.isArray(adapter.models) &&
        adapter.models.every(
          (model: unknown) => object(model) && typeof model.id === "string" && typeof model.display_name === "string",
        ),
    )
  );
}

export function isSupporterStatus(value: unknown): value is SupporterStatus {
  if (
    !object(value) ||
    !Array.isArray(value.connections) ||
    !object(value.budget) ||
    !object(value.spend) ||
    !object(value.last_verdict)
  )
    return false;
  if (typeof value.connected !== "boolean") return false;
  if (value.adapter !== null && typeof value.adapter !== "string") return false;
  if (
    typeof value.calls_last_hour !== "number" ||
    typeof value.calls_last_day !== "number" ||
    typeof value.deadline_ms !== "number"
  )
    return false;
  for (const period of ["today", "month"]) {
    const totals = value.spend[period];
    if (!object(totals) || typeof totals.input_tokens !== "number" || typeof totals.output_tokens !== "number")
      return false;
  }
  return value.connections.every(
    (row: unknown) =>
      object(row) &&
      typeof row.adapter === "string" &&
      typeof row.active === "boolean" &&
      object(row.token) &&
      typeof row.token.present === "boolean",
  );
}

export function redactSupporterError(message: string, secret?: string): string {
  if (!secret) return message;
  for (const value of [secret, secret.trim()]
    .filter(Boolean)
    .flatMap((value) => [value, JSON.stringify(value).slice(1, -1), encodeURIComponent(value)])) {
    message = message.split(value).join("[redacted]");
  }
  return message;
}
