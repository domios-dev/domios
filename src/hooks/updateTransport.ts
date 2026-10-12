import { nativeBridge, NativeCapabilityUnavailable } from "../lib/nativeBridge";
import { daemonFetch, type DaemonApiConfig } from "../lib/webApi";

export function requestUpdateChannel(api: DaemonApiConfig, channel?: string): Promise<Response> {
  return daemonFetch(api, "/api/update/channel", channel === undefined ? {} : {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ channel }),
  });
}

export function checkForUpdate(force: boolean): Promise<unknown> {
  return nativeBridge.updater?.check(force) ?? Promise.reject(new NativeCapabilityUnavailable("app updater"));
}

export function getUpdateStatus(): Promise<unknown> {
  return nativeBridge.updater?.status() ?? Promise.reject(new NativeCapabilityUnavailable("app updater"));
}

export function installAppUpdate(): Promise<unknown> {
  return nativeBridge.updater?.install() ?? Promise.reject(new NativeCapabilityUnavailable("app updater"));
}
