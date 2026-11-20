import { invoke } from "../../lib/bridge";
import type { Service, ActivityEvent } from "./types";

export async function servicesCreate(
  input: Partial<Service>,
): Promise<{ service: Service; token: string }> {
  return invoke("services_create", { input });
}

export async function activityList(opts?: {
  source?: string;
  since?: number;
  limit?: number;
}): Promise<ActivityEvent[]> {
  return invoke("activity_list", opts ?? {});
}

export async function servicesSmartAddInspect(url: string): Promise<{
  suggested_name?: string;
  recommended_health_mode: string;
  recommended_health_path?: string;
  field_mapping: Record<string, string>;
  raw_response: unknown;
}> {
  return invoke("services_smart_add_inspect", { url });
}
