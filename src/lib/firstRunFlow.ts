export type FirstRunPhase = "choosing" | "installing" | "starting" | "ready" | "failed";
export type FirstRunEvent = "install" | "start" | "installed" | "ready" | "failure";

export function firstRunTransition(phase: FirstRunPhase, event: FirstRunEvent): FirstRunPhase {
  if (event === "failure") return "failed";
  if (phase === "choosing" || phase === "failed") {
    if (event === "install") return "installing";
    if (event === "start") return "starting";
  }
  if (phase === "installing" && event === "installed") return "starting";
  if (phase === "starting" && event === "ready") return "ready";
  return phase;
}
