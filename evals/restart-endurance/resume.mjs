export function resumeSetup(result, launcherStarted, resumedAt) {
  if (!result.setup_complete || result.rounds.some((round) => round.round > 0) || launcherStarted) {
    throw new Error("Only a completed setup before the first launcher invocation can continue.");
  }
  if (result.fatal) {
    (result.harness_failures ??= []).push({ reason: result.fatal, resumed_at: resumedAt,
      before_round: result.round_started_at, pre_restart_tabs: result.pre_restart_tabs });
    delete result.fatal;
  }
  return result;
}
