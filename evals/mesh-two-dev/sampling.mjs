export function validateTarget(instance, port) {
  if (!((instance === 'aiderqa' && port === 7697) || (instance === 'meshqa' && port === 7698))) {
    throw new Error('Only aiderqa:7697 and meshqa:7698 are allowed');
  }
}
export function sampleCommands(tab, session, peer) {
  if (!/^[a-zA-Z0-9_-]+$/.test(tab) || !/^[a-zA-Z0-9_-]+$/.test(session)) throw new Error('Invalid identity');
  if (peer && !/^[a-f0-9-]{6,}$/.test(peer)) throw new Error('Exact peer identity required');
  const target = peer ? ['--mesh', peer] : [];
  return [
    ['tab', 'inspect', '--tab', tab, '--json', ...target],
    ['tab', 'status', '--tab', tab, '--json', ...target],
    ['tab', 'chat', '--tab', tab, '--json', ...target],
    ['tab', 'get-text', '--tab', tab, '--json', ...target],
    ['sessions', 'get', session, '--json', ...target],
  ];
}
export const osSamples = {
  macos: ['ps', '-o', 'pid=,rss=,thcount=', '-p', '<verified-DEV-pid>'],
  windows: ['powershell', '-NoProfile', '-Command', 'Get-Process -Id <verified-DEV-pid> | Select-Object Id,WorkingSet64,HandleCount,@{n="Threads";e={$_.Threads.Count}} | ConvertTo-Json'],
};
export async function waitForEvent(subscribe, predicate, deadlineMs = 20000) {
  const signal = AbortSignal.timeout(deadlineMs);
  for await (const event of subscribe(signal)) {
    signal.throwIfAborted();
    if (predicate(event)) return event;
  }
  throw new Error('Event stream ended before required state');
}
