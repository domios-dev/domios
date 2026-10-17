// Caller: node --test scripts/sccache-server-process.test.mjs
import assert from 'node:assert/strict';
import test from 'node:test';
import { parseWindowsProcess, parsePosixProcesses, serverProcessSnapshot } from './sccache-server-process.mjs';

test('parses the Windows listener process identity', () => {
  assert.deepEqual(parseWindowsProcess('{"pid":42,"started_at":"2026-10-17T08:00:00Z"}'), { pid: 42, started_at: '2026-10-17T08:00:00Z' });
  assert.equal(parseWindowsProcess(''), null);
  assert.equal(parseWindowsProcess('{"pid":"bad"}'), null);
});

test('filters POSIX process listings to sccache and retains start time', () => {
  assert.deepEqual(parsePosixProcesses('  42 Fri Oct 17 08:29:53 2026 /usr/bin/sccache\n  43 Fri Oct 17 08:30:00 2026 cargo\n'), [
    { pid: 42, started_at: 'Fri Oct 17 08:29:53 2026' },
  ]);
});

test('queries the endpoint owner on Windows', () => {
  let observed;
  const result = serverProcessSnapshot({ platform: 'win32', env: { SCCACHE_SERVER_PORT: '4227' }, run: (command, args) => {
    observed = { command, args };
    return { status: 0, stdout: '{"pid":72,"started_at":"2026-10-17T08:00:00Z"}' };
  } });
  assert.equal(result.processes[0].pid, 72);
  assert.match(observed.args.at(-1), /LocalPort 4227/);
});

test('queries process names on POSIX', () => {
  const result = serverProcessSnapshot({ platform: 'linux', run: (command, args) => {
    assert.equal(command, 'ps');
    assert.deepEqual(args, ['-A', '-o', 'pid=,lstart=,comm=']);
    return { status: 0, stdout: '  72 Fri Oct 17 08:00:00 2026 sccache\n' };
  } });
  assert.deepEqual(result, { available: true, processes: [{ pid: 72, started_at: 'Fri Oct 17 08:00:00 2026' }] });
});
