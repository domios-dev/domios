import type { CronJob } from "../../lib/types";

export function cronRowMenu(job: Pick<CronJob, "enabled" | "protected">) {
  return [
    { action: "toggle", label: job.enabled ? "Disable" : "Enable", disabled: false },
    job.protected
      ? { action: "delete", label: "System job cannot be deleted", disabled: true }
      : { action: "delete", label: "Delete cron", disabled: false },
  ] as const;
}
