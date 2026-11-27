import { localFetch } from "../webApi";
import type {
  CloudPackCheckoutReply,
  CloudPromoRedemption,
  CloudReceipt,
  CloudUsagePayload,
} from "./types";

export class CloudApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryAfterSeconds?: number;
  readonly bytesWritten?: boolean;

  constructor(options: {
    status: number;
    code: string;
    message: string;
    retryAfterSeconds?: number;
    bytesWritten?: boolean;
  }) {
    super(options.message);
    this.name = "CloudApiError";
    this.status = options.status;
    this.code = options.code;
    this.retryAfterSeconds = options.retryAfterSeconds;
    this.bytesWritten = options.bytesWritten;
  }
}

function parseApiError(status: number, text: string): CloudApiError {
  try {
    const parsed: unknown = JSON.parse(text);
    if (parsed && typeof parsed === "object" && "error" in parsed) {
      const err = (parsed as { error?: unknown }).error;
      if (err && typeof err === "object") {
        const record = err as Record<string, unknown>;
        return new CloudApiError({
          status,
          code: typeof record.code === "string" ? record.code : "cloud_error",
          message: typeof record.message === "string" ? record.message : "Cloud request failed",
          retryAfterSeconds: typeof record.retry_after_seconds === "number" ? record.retry_after_seconds : undefined,
          bytesWritten: typeof record.bytes_written === "boolean" ? record.bytes_written : undefined,
        });
      }
    }
  } catch {
    // Non-JSON error body falls through to generic error below.
  }
  return new CloudApiError({
    status,
    code: status === 404 ? "route_unavailable" : "cloud_error",
    message: text.trim() || `Request failed (${status})`,
  });
}

async function cloudRequest<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const response = await localFetch(path, {
    method,
    signal,
    ...(body !== undefined
      ? { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
      : {}),
  });
  const text = await response.text();
  if (!response.ok) throw parseApiError(response.status, text);
  return text ? (JSON.parse(text) as T) : ({} as T);
}

export async function listCloudMachines(signal?: AbortSignal): Promise<CloudReceipt[]> {
  const reply = await cloudRequest<{ machines?: CloudReceipt[] }>("GET", "/api/mesh/cloud/list", undefined, signal);
  return Array.isArray(reply.machines) ? reply.machines : [];
}

export function createCloudMachine(
  params: { size: string; name?: string; retry_machine_id?: string },
  signal?: AbortSignal,
): Promise<CloudReceipt> {
  return cloudRequest<CloudReceipt>("POST", "/api/mesh/cloud/create", params, signal);
}

export function sleepCloudMachine(id: string, signal?: AbortSignal): Promise<CloudReceipt> {
  return cloudRequest<CloudReceipt>("POST", `/api/mesh/cloud/${encodeURIComponent(id)}/sleep`, {}, signal);
}

export function archiveCloudMachine(id: string, signal?: AbortSignal): Promise<CloudReceipt> {
  return cloudRequest<CloudReceipt>("POST", `/api/mesh/cloud/${encodeURIComponent(id)}/archive`, {}, signal);
}

export function wakeCloudMachine(id: string, signal?: AbortSignal): Promise<CloudReceipt> {
  return cloudRequest<CloudReceipt>("POST", `/api/mesh/cloud/${encodeURIComponent(id)}/wake`, {}, signal);
}

export function deleteCloudMachine(id: string, signal?: AbortSignal): Promise<CloudReceipt> {
  return cloudRequest<CloudReceipt>("POST", `/api/mesh/cloud/${encodeURIComponent(id)}/delete`, { confirm: true }, signal);
}

export async function getCloudUsage(signal?: AbortSignal): Promise<CloudUsagePayload> {
  const payload = await cloudRequest<unknown>("GET", "/api/mesh/cloud/usage", undefined, signal);
  if (
    !payload ||
    typeof payload !== "object" ||
    typeof (payload as Record<string, unknown>).remaining !== "number" ||
    typeof (payload as Record<string, unknown>).used !== "number"
  ) {
    throw new CloudApiError({ status: 502, code: "invalid_usage", message: "Cloud usage payload unavailable" });
  }
  return payload as CloudUsagePayload;
}

export function redeemCloudPromo(code: string, signal?: AbortSignal): Promise<CloudPromoRedemption> {
  return cloudRequest<CloudPromoRedemption>("POST", "/api/mesh/cloud/redeem", { code }, signal);
}

export function createCloudPackCheckout(
  input: { pack_id: "custom"; amount_cents: number } | { pack_id: string },
  signal?: AbortSignal,
): Promise<CloudPackCheckoutReply> {
  return cloudRequest<CloudPackCheckoutReply>("POST", "/api/mesh/cloud/checkout", input, signal);
}
