import type { ProviderCliStatus } from "./providerCli";

export type ProviderUpdateModel = {
  key: string;
  state: "available" | "running" | "succeeded" | "failed" | "unsupported";
  message: string;
  canUpdate: boolean;
};

export function providerUpdateModel(provider: string, status: ProviderCliStatus | null): ProviderUpdateModel | null {
  if (!status?.required || status.installed !== true) return null;
  const job = status.update_job;
  if (job) return {
    key: `${job.tab_id}:${job.state}`, state: job.state, message: job.summary,
    canUpdate: job.state === "failed" && !!status.updater && !!status.latest_version,
  };
  if (!status.latest_version) return null;
  return {
    key: status.latest_version,
    state: status.updater ? "available" : "unsupported",
    message: status.updater
      ? `New ${provider} version ${status.latest_version} is out — update?`
      : `${provider} ${status.latest_version} is available. ${status.update_refusal ?? "Update using the original installer."}`,
    canUpdate: !!status.updater,
  };
}
