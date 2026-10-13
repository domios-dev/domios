import { invoke } from "./bridge";

export interface UpdateAvailabilityItem {
  id: string;
  kind: "plugin" | "app" | "frontend";
  installed: string | null;
  latest: string | null;
  compatible: boolean;
  state: "up_to_date" | "update_available" | "incompatible" | "core_silent";
  reason: string;
  channel: string | null;
}

export interface UpdateAvailabilitySnapshot {
  items: UpdateAvailabilityItem[];
  errors: Record<string, string>;
}

export function checkUpdateAvailability(force = false): Promise<UpdateAvailabilitySnapshot> {
  return invoke<UpdateAvailabilitySnapshot>("check_update_availability", { force });
}
