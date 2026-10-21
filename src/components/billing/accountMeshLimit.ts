import { freeLimitGate } from "./freeLimitGate";
import { meshMachinesRefusal } from "./freeLimitModel";

/** One Account Mesh request's structured server refusal, as the account-Mesh facade delivers it. */
export interface AccountMeshRefusalNotice {
  requestId: string;
  refusal: unknown;
}

/** The facade rejects a refused enroll/link with this, so Add flows never classify from message text. */
export class AccountMeshRefusalError extends Error {
  constructor(readonly notice: AccountMeshRefusalNotice) {
    super("Account Mesh request refused");
  }
}

const REMEMBERED_REQUESTS = 64;
const prompted = new Set<string>();

const isRequestId = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 128;

/** Opens the plans modal once per refused request; false for every refusal that is not the server's machine limit. */
export function raiseMeshMachineLimit(notice: AccountMeshRefusalNotice): boolean {
  const limit = meshMachinesRefusal(notice.refusal);
  if (!limit || !isRequestId(notice.requestId)) return false;
  // A replayed request keeps its earlier prompt or dismissal.
  if (prompted.has(notice.requestId)) return true;
  prompted.add(notice.requestId);
  if (prompted.size > REMEMBERED_REQUESTS) prompted.delete(prompted.values().next().value!);
  freeLimitGate.openMeshMachines(notice.requestId, limit);
  return true;
}

/** A user Add or retry: the machine-limit refusal opens the modal and resolves undefined; every other error still rejects. */
export async function openOnMeshRefusal<T>(request: () => Promise<T>): Promise<T | undefined> {
  try {
    return await request();
  } catch (error) {
    if (error instanceof AccountMeshRefusalError && raiseMeshMachineLimit(error.notice)) return undefined;
    throw error;
  }
}
