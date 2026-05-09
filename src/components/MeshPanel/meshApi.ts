import { invoke } from "@tauri-apps/api/core";
import type { Service, ActivityEvent, ServiceKind } from "./types";

export async function servicesList(filter?: {
  kind?: ServiceKind;
  enabled?: boolean;
}): Promise<Service[]> {
  return invoke("services_list", filter ?? {});
}

export async function servicesCreate(
  input: Partial<Service>,
): Promise<{ service: Service; token: string }> {
  return invoke("services_create", { input });
}

export async function servicesUpdate(
  id: string,
  patch: Partial<Service>,
): Promise<Service> {
  return invoke("services_update", { id, patch });
}

export async function servicesDelete(id: string): Promise<void> {
  return invoke("services_delete", { id });
}

export async function servicesRegenerateToken(id: string): Promise<string> {
  return invoke("services_regenerate_token", { id });
}

export async function activityList(opts?: {
  source?: string;
  since?: number;
  limit?: number;
}): Promise<ActivityEvent[]> {
  return invoke("activity_list", opts ?? {});
}
