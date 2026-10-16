export function workloadDuration(value) {
  if (value === undefined) return null;
  const seconds = Number(value);
  if (!Number.isSafeInteger(seconds) || seconds < 1 || seconds > 600) throw Error('Seconds must be 1..600');
  return seconds * 1000;
}

export function continueWorkload({ duration, started, now, cycle, cycles }) {
  return duration === null ? cycle <= cycles : now - started < duration && cycle <= 10000;
}

export function sampleWorkload({ duration, now, lastSample }) {
  return duration === null || now - lastSample >= 60000;
}
