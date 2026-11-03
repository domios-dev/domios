export function outputLimit(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  const details = record.incomplete_details as Record<string, unknown> | undefined;
  if (record.stop_reason === "max_tokens" || record.finish_reason === "length") return true;
  if (record.status === "incomplete" && details?.reason === "max_output_tokens") return true;
  const message = record.message;
  return typeof message === "string" && message.length <= 16384
    && /incomplete response[^\n]*\bmax_output_tokens\b/i.test(message);
}
