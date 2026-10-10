export function restartObserved(before, after) {
  return Number.isInteger(before?.pid) && Number.isInteger(after?.pid) && before.pid !== after.pid &&
    Number.isSafeInteger(before.started_us) && Number.isSafeInteger(after.started_us) &&
    after.started_us > before.started_us;
}

export async function launchRestart({ snapshot, stop, start, record = () => {} }) {
  const evidence = { pass: false, phase: "before_stop" };
  try {
    evidence.before = await snapshot();
    record(evidence);
    evidence.phase = "stopping";
    record(evidence);
    await stop();
    evidence.phase = "starting";
    record(evidence);
    await start();
    evidence.after = await snapshot();
    evidence.pass = restartObserved(evidence.before, evidence.after);
    evidence.reason = evidence.pass ? undefined : "restart_not_observed";
    evidence.phase = "verified";
  } catch (error) {
    evidence.reason = `restart_not_observed: ${error.message}`;
  }
  record(evidence);
  return evidence;
}
