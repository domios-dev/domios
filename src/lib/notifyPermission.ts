/**
 * Permission for the desktop toast.
 *
 * It is established when the surface is switched on, never at send time, and
 * never assumed: a host that cannot answer reads `unknown`, which is not a
 * grant and not a refusal. Only a refusal is allowed to contradict the stored
 * config, because a switch that reads on while the OS refuses is the same lie
 * as a send whose Result was dropped.
 */
import { invoke } from "./bridge";

export type NotifyPermission = "granted" | "denied" | "prompt" | "unknown";

function asPermission(value: unknown): NotifyPermission {
  return value === "granted" || value === "denied" || value === "prompt" ? value : "unknown";
}

/** What the OS says now. Reading must never raise a prompt. */
export async function readNotifyPermission(): Promise<NotifyPermission> {
  try {
    return asPermission(await invoke<string>("notify_permission_state"));
  } catch {
    return "unknown";
  }
}

/** The enabling gesture. On asks the OS; off asks nothing and reports what is known. */
export async function establishNotifyPermission(enabling: boolean): Promise<NotifyPermission> {
  try {
    return asPermission(await invoke<string>("establish_notify_permission", { enabling }));
  } catch {
    return "unknown";
  }
}

/** What the switch may read: opted in, and not refused by the system. */
export function desktopSurfaceReadsOn(
  configured: boolean | undefined,
  permission: NotifyPermission,
): boolean {
  return configured === true && permission !== "denied";
}
