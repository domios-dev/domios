import { invoke } from "../lib/bridge";
import { daemonFetch, type DaemonApiConfig } from "../lib/webApi";

export function requestUpdateChannel(api: DaemonApiConfig, channel?: string): Promise<Response> {
  return daemonFetch(api, "/api/update/channel", channel === undefined ? {} : {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ channel }),
  });
}

export function checkForUpdate(force: boolean): Promise<unknown> {
  return invoke("check_for_update", { force });
}

export function getUpdateStatus(): Promise<unknown> {
  return invoke("get_update_status");
}

export function installAppUpdate(): Promise<unknown> {
  return invoke("install_app_update");
}
