import { invoke } from "../../lib/bridge";
import type { CronJob, CronRunInfo } from "../../lib/types";
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

export async function cronGet(id: string): Promise<CronJob> {
  return invoke("cron_get", { id });
}

export async function cronHistory(id: string): Promise<CronRunInfo[]> {
  return invoke("cron_history", { id });
}

export async function cronAction(
  id: string,
  action: "enable" | "disable" | "trigger" | "cancel",
): Promise<unknown> {
  if (action === "trigger") return invoke("cron_trigger", { id });
  if (action === "cancel") return invoke("cron_cancel", { id });
  return invoke("cron_toggle", { id, enabled: action === "enable" });
}

export async function cronUpdate(id: string, patch: Record<string, unknown>): Promise<unknown> {
  return invoke("cron_update", { id, ...patch });
}

export async function cronDelete(id: string): Promise<unknown> {
  return invoke("cron_delete", { id });
}

export async function peerCronGet(instanceId: string, id: string): Promise<CronJob> {
  return invoke("mesh_peer_cron_get", { instanceId, id });
}

export async function peerCronHistory(instanceId: string, id: string): Promise<CronRunInfo[]> {
  return invoke("mesh_peer_cron_history", { instanceId, id });
}

export async function peerCronAction(
  instanceId: string,
  id: string,
  action: "enable" | "disable" | "trigger" | "cancel",
): Promise<unknown> {
  return invoke("mesh_peer_cron_action", { instanceId, id, action });
}

export async function peerCronUpdate(
  instanceId: string,
  id: string,
  patch: Record<string, unknown>,
): Promise<unknown> {
  return invoke("mesh_peer_cron_update", { instanceId, id, patch });
}

export async function peerCronDelete(instanceId: string, id: string): Promise<unknown> {
  return invoke("mesh_peer_cron_delete", { instanceId, id });
}

export async function peerServiceDelete(instanceId: string, id: string): Promise<unknown> {
  return invoke("mesh_peer_service_delete", { instanceId, id });
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
