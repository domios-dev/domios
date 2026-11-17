import { probeReady } from "./oracle.mjs";

const terminalStages = new Set(["spawn_failed", "awaiting_owner_prompt"]);

export function launchFailure(record) {
  if (!record) return "Tab absent from the all-tabs projection";
  if (terminalStages.has(record.boot_stage)) return `${record.boot_stage}: ${record.boot_failure_reason ?? JSON.stringify(record.pending_prompt ?? null)}`;
  return null;
}

export function restoreReady(record, fixture) {
  return record?.boot_stage === "agent_ready" &&
    probeReady(record, fixture.first_turn_policy, Boolean(fixture.has_answer));
}

export function reconcileReadiness(readiness, settled, fixtures) {
  return readiness.map((observed) => {
    if (!observed.readiness_error) return { ...observed };
    const fixture = fixtures.find((fixture) => fixture.id === observed.id);
    const current = settled.find((record) => record.id === observed.id);
    return fixture && restoreReady(current, fixture)
      ? { ...current, recovered_readiness_error: observed.readiness_error }
      : { ...observed };
  });
}
