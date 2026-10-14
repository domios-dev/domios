import type { UpdateAvailabilitySnapshot } from "./updateAvailability";

export function classifyUpdateStatus(snapshot: UpdateAvailabilitySnapshot) {
  const information = new Map<string, string>();
  for (const item of snapshot.items) {
    if (item.kind === "plugin" && item.reason === "local_ahead") {
      information.set(item.id, `${item.id}: installed version ${item.installed ?? "unknown"} is ahead of channel version ${item.latest ?? "unknown"}`);
    }
  }
  const errors: Record<string, string> = {};
  for (const [source, message] of Object.entries(snapshot.errors)) {
    const failures: string[] = [];
    for (const part of message.split("; ")) {
      const ahead = /^system plugin '([^']+)' is not installable: local_ahead$/.exec(part);
      if (ahead) {
        if (!information.has(ahead[1])) information.set(ahead[1], `${ahead[1]}: installed version is ahead of channel`);
      } else failures.push(part);
    }
    if (failures.length) errors[source] = failures.join("; ");
  }
  return { errors, information: [...information.values()] };
}

export type UpdateFailureOrigin = "background" | "check" | "update";
export interface UpdateFailure { message: string; details: string }

export function classifyUpdateFailure(origin: UpdateFailureOrigin, detail: unknown): UpdateFailure | null {
  if (origin === "background") return null;
  return {
    message: origin === "check" ? "Couldn't check for updates. Try again later." : "Couldn't update. Try again later.",
    details: detail instanceof Error ? detail.toString() : String(detail),
  };
}
