import type { ErrorDescription } from "../../lib/describeError";
import { NeighborRuntimeFailure } from "./neighborRuntimeClient";

const MAX_DIAGNOSTIC = 256;

function diagnostic(message: string): string {
  // Bound work before redaction, then bound the displayed result, including its truncation marker.
  const text = message.slice(0, 2048)
    .replace(/\b(?:sk-[a-z0-9_-]+|gh[pousr]_[a-z0-9]+|eyJ[a-z0-9_-]+\.[a-z0-9_-]+\.[a-z0-9_-]+)/gi, "[redacted]")
    .replace(/https?:\/\/[^\s<>"']+/gi, "[redacted]")
    .replace(/\b(?:[a-z0-9]+[_-])*(?:authorization|bearer|password|passwd|secret|token|api[_ -]?key|verification[_ -]?code|device[_ -]?code|otp)["']?\s*[:= ]\s*[^\r\n]+/gi, "[redacted]")
    .replace(/[\u0000-\u001f\u007f]/g, " ").trim();
  return text.length > MAX_DIAGNOSTIC || message.length > 2048
    ? `${text.slice(0, MAX_DIAGNOSTIC - 1)}…` : text;
}

export function receiverFailureDescription(failure: unknown): ErrorDescription {
  const typed = failure instanceof NeighborRuntimeFailure;
  const message = typed
    ? failure.reason === "unavailable" ? "The receiving service is unavailable. Try again."
      : diagnostic(failure.message) || "Receiving could not move here."
    : "Receiving could not move here. Try again.";
  const cause = typed ? `${failure.reason}: ${message}` : message;
  return { title: "Receiving could not move here", cause, raw: cause, actions: [] };
}
