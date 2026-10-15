export const HEALTH_WARNING_MS = 5000;
export const HEALTH_DEADLINE_MS = 30000;

export function classifyPreflight({ phase, at, ms, health, error }) {
  const failed = Boolean(error) || ms > HEALTH_WARNING_MS;
  return { id: failed ? 'api_unresponsive' : 'api_health', phase, at, ms,
    verdict: failed ? 'fail' : 'pass', threshold_ms: HEALTH_WARNING_MS, deadline_ms: HEALTH_DEADLINE_MS,
    responded: health != null, error: error ?? null, health: health ?? null };
}

export async function healthPreflight({ read, phase = 'entry', now = Date.now }) {
  const at = now();
  try {
    const health = await read(AbortSignal.timeout(HEALTH_DEADLINE_MS));
    return classifyPreflight({ phase, at, ms: now() - at, health });
  } catch (error) {
    return classifyPreflight({ phase, at, ms: now() - at, error: error.message });
  }
}

export function assertDevHealth(health) {
  if (health?.status !== 'ok' || health.instance?.name !== '' || Number(health.instance?.api_port) !== 7686 ||
      Number(health.instance?.vite_port) !== 1420 || Number(health.instance?.cdp_port) !== 9223)
    throw new Error('Refused: health identity does not match default DEV 7686/1420/9223');
}
