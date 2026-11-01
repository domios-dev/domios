const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);

export function decodeCliResult(stdout) {
  const envelope = JSON.parse(stdout);
  if (!object(envelope) || !Object.hasOwn(envelope, 'result')
    || Object.keys(envelope).some(key => !['result', 'meta', 'next', 'warnings'].includes(key))) {
    throw new Error('CLI result requires the current unambiguous result envelope');
  }
  const payload = envelope.result;
  if (payload === null || (object(payload) && (payload.ok === false || payload.error != null))) {
    throw new Error('CLI result is absent or reports an error');
  }
  return payload;
}

export async function cliResult(execute, ...args) {
  const result = await execute(...args);
  return decodeCliResult(result.stdout);
}
