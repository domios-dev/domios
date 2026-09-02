import { invoke } from "../lib/bridge";

export function checkForUpdate(force: boolean): Promise<unknown> {
  return invoke("check_for_update", { force });
}

export function getUpdateStatus(): Promise<unknown> {
  return invoke("get_update_status");
}

export function installAppUpdate(): Promise<unknown> {
  return invoke("install_app_update");
}
