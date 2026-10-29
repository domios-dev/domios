import type { MediaContentType } from "./mediaDelivery";

export type BinaryContentType = MediaContentType | "application/octet-stream";
export interface BinaryTransferDescriptor {
  maxUploadBytes?: number;
  maxDownloadBytes?: number;
  timeoutMs?: number;
  credentialOrigins?: Record<string, string[]>;
  requestHeaders?: string[];
}
export type TransferCredential =
  | { kind: "secret"; name: string; header: string; prefix?: string }
  | { kind: "declared"; id: string; headers: Record<string, string> };
export interface TransferFileIdentity { size: number; sha256: string; }
export interface TransferInputGrant { handle: string; fileName: string; contentType: BinaryContentType; size: number; sha256: string; }
export type TransferInput =
  | { kind: "owned"; path: string; contentType: BinaryContentType; fileName: string; identity?: TransferFileIdentity }
  | { kind: "selected"; handle: string };
export type TransferPart =
  | { kind: "text"; name: string; value: string }
  | { kind: "file"; name: string; file: TransferInput };
export interface TransferRequest {
  url: string;
  method: "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE";
  headers?: Record<string, string>;
  credential?: TransferCredential;
  body?: { kind: "json"; value: unknown } | { kind: "file"; file: TransferInput } | { kind: "multipart"; parts: TransferPart[] };
  response:
    | { kind: "file"; fileName: string; contentType: BinaryContentType; maxBytes: number; sha256?: string; statuses: number[] }
    | { kind: "json"; maxBytes: number; statuses: number[] }
    | { kind: "empty"; statuses: number[] };
  timeoutMs?: number;
}
export type TransferRefusal = "invalid_request" | "unavailable" | "not_allowed" | "aborted" | "deadline" | "too_large" | "mime_mismatch" | "digest_mismatch" | "file_changed" | "http_status" | "redirect_denied" | "transport_unconfirmed" | "invalid_response" | "storage";
export interface TransferFailure {
  error: TransferRefusal;
  /** Conservative dispatch evidence, never proof of remote completion or provider cancellation. */
  requestMayHaveStarted: boolean;
  status: number | null;
}
export type TransferReceipt =
  | { kind: "file"; status: number; file: { path: string; fileName: string; contentType: BinaryContentType; size: number; sha256: string } }
  | { kind: "json"; status: number; data: unknown; size: number; sha256: string }
  | { kind: "empty"; status: number };
export type TransferResult = TransferReceipt | TransferFailure;
export type TransferStart = { jobId: string } | TransferFailure;
export interface HostTransfer {
  bindInput<T>(handle: string, destinationOrigin: string, then: (result: TransferInputGrant | TransferFailure) => T): T;
  start(request: TransferRequest): TransferStart;
  async<T>(request: TransferRequest, then: (result: TransferResult) => T): T;
  abort(jobId: string): boolean;
}
