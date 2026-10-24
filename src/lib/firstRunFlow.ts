export type FirstRunPhase = "choosing" | "installing" | "signIn" | "starting" | "ready" | "failed";
export type FirstRunEvent = "install" | "start" | "installed" | "authenticated" | "ready" | "failure";

export function firstRunTransition(phase: FirstRunPhase, event: FirstRunEvent): FirstRunPhase {
  if (event === "failure") return "failed";
  if (phase === "choosing" || phase === "failed") {
    if (event === "install") return "installing";
    if (event === "start") return "signIn";
  }
  if (phase === "installing" && event === "installed") return "signIn";
  if (phase === "signIn" && event === "authenticated") return "starting";
  if (phase === "starting" && event === "ready") return "ready";
  return phase;
}
