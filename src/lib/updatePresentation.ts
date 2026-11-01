import type { UpdatePhase, UpdateStatus } from "../hooks/useUpdate";

export function updateStatusForPhase(phase: UpdatePhase, installing: boolean): UpdateStatus | null {
  switch (phase) {
    case "checking": return installing ? "checking" : null;
    case "downloading": return "downloading";
    case "applying": return "applying";
    case "success": return "restarting";
    case "error": return "error";
    case "idle": return null;
  }
}

export function updatePresentation(status: UpdateStatus, installing: boolean, version: string | null) {
  const blocking = status === "applying" || status === "restarting";
  const retry = status === "error";
  const background = !blocking && (retry || (installing && (status === "checking" || status === "downloading")));
  const title = blocking
    ? `Restarting to install ${version ?? "the update"}…`
    : retry ? "Update failed"
    : status === "downloading" ? "Downloading and preparing update"
    : "Preparing update";
  return { blocking, background, retry, title };
}
