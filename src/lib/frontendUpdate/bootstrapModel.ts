import { TransportError } from "../transportClient";

export type FrontendUpdateFailureReason = "invalid-client" | "checkpoint-unavailable" | "status-unavailable" | "api-incompatible" | "storage-unavailable" | "boot-context-invalid" | "unexpected";
export class FrontendUpdateError extends Error {
  constructor(public reason: FrontendUpdateFailureReason, public cause?: unknown) {
    super(reason);
    this.name = "FrontendUpdateError";
  }
}

export function frontendCheckpointClientId(identity: string): string {
  const clientId = identity.replace(/^client:/, "client-");
  if (identity === "client:" || !/^[a-zA-Z0-9_-]{1,128}$/.test(clientId)) throw new FrontendUpdateError("invalid-client");
  return clientId;
}

export function frontendUpdateDiagnostic(error: unknown): { name: string; reason: FrontendUpdateFailureReason; httpStatus?: number } {
  const name = error instanceof Error && ["Error", "ReferenceError", "TypeError", "SyntaxError", "SecurityError", "FrontendUpdateError"].includes(error.name) ? error.name : "Error";
  const reason = error instanceof FrontendUpdateError ? error.reason
    : name === "SecurityError" ? "storage-unavailable"
    : name === "SyntaxError" ? "boot-context-invalid" : "unexpected";
  const cause = error instanceof FrontendUpdateError ? error.cause : error;
  return { name, reason, ...(cause instanceof TransportError && Number.isInteger(cause.status) && cause.status >= 100 && cause.status <= 599 ? { httpStatus: cause.status } : {}) };
}
