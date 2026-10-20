import { invoke } from "./bridge";

export interface UpdateAvailabilityItem {
  id: string;
  kind: "plugin" | "app" | "frontend";
  installed: string | null;
  latest: string | null;
  compatible: boolean;
  state: "up_to_date" | "update_available" | "ready_to_apply" | "incompatible" | "core_silent";
  reason: string;
  channel: string | null;
}

export interface UpdateAvailabilitySnapshot {
  items: UpdateAvailabilityItem[];
  errors: Record<string, string>;
}

export function decodeUpdateAvailability(value: unknown): UpdateAvailabilitySnapshot {
  if (!value || typeof value !== "object" || !("items" in value) || !Array.isArray(value.items)
    || !("errors" in value) || !value.errors || typeof value.errors !== "object" || Array.isArray(value.errors)
    || !Object.values(value.errors).every(error => typeof error === "string")
    || !value.items.every(isAvailabilityItem)) throw new Error("Invalid update availability response");
  return value as UpdateAvailabilitySnapshot;
}

function isAvailabilityItem(value: unknown): value is UpdateAvailabilityItem {
  if (!value || typeof value !== "object") return false;
  const item = value as Record<string, unknown>;
  const nullableString = (field: unknown) => field === null || typeof field === "string";
  return typeof item.id === "string" && typeof item.kind === "string" && ["plugin", "app", "frontend"].includes(item.kind)
    && nullableString(item.installed) && nullableString(item.latest) && nullableString(item.channel)
    && typeof item.compatible === "boolean" && typeof item.reason === "string"
    && typeof item.state === "string" && ["up_to_date", "update_available", "ready_to_apply", "incompatible", "core_silent"].includes(item.state);
}

export async function checkUpdateAvailability(force = false): Promise<UpdateAvailabilitySnapshot> {
  return decodeUpdateAvailability(await invoke<unknown>("check_update_availability", { force }));
}
