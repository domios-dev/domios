import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import path from 'node:path';

const bash = process.platform === 'win32' ? path.join(process.env.ProgramFiles, 'Git', 'bin', 'bash.exe') : 'bash';

const build = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const source = build.split("<<'LOCK_STORM_PY'\n")[1].split('\nLOCK_STORM_PY')[0];

test('lock storm aggregates percentiles, timestamped sites, statuses and persisted JSON', () => {
  const checks = `
import contextlib, io, json, pathlib, tempfile, unittest
scope = {"__name__": "lock_storm_fixture"}
exec(${JSON.stringify(source)}, scope)
class StormTests(unittest.TestCase):
    def test_percentiles(self):
        self.assertEqual(scope['percentile']([9, 1, 5, 3], .5), 3)
        self.assertEqual(scope['percentile']([9, 1, 5, 3], .95), 9)
        self.assertIsNone(scope['percentile']([], .5))
    def test_log_window_and_histograms(self):
        line = '2026-10-15T12:00:00Z [DEBUG] [LOCK-HOLD] pty.lock() held 5961ms (waited 12ms) at service/spawn_shell.rs:243'
        start = 1792065600
        result = scope['lock_histogram']([line, line.replace('12:00:00', '11:59:59'), line.replace('pty.lock()', 'mux.read()'), line.replace('pty.lock()', 'launch_registry.lock()'), line.replace(' (waited 12ms)', '')], start + .1, start + .9)
        self.assertEqual(result['per_lock'], {'pty': 1, 'mux': 1, 'launch_registry': 1})
        self.assertEqual(result['per_site']['pty at service/spawn_shell.rs:243'], 1)
        self.assertEqual(result['unmatched'], 1)
    def test_worker_http_and_transport_status(self):
        import subprocess
        worker = scope['WORKER'].replace('source evals/_lib/lib.sh', '''dev_get() {
case "$1" in
  /ok) echo '[dev-http] GET /ok -> HTTP 200' >&2 ;;
  /bad) echo '[dev-http] GET /bad -> HTTP 503' >&2; echo 'ERROR [dev-http]: GET /bad -> HTTP 503' >&2; return 22 ;;
  *) return 7 ;;
esac
}''')
        result = subprocess.run([${JSON.stringify(bash)}, '-c', worker], input='/ok\\n/bad\\n/down\\n', capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['0 200', '22 503', '7 transport_error'])
    def test_persisted_result(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / 'log').write_text('')
            scope['sys'].argv = ['fixture', '1', str(root / 'out.json'), 'existing-tab', str(root / 'log')]
            scope['measure'] = lambda route, deadline: {'samples': 2, 'p50_ms': 1, 'p95_ms': 2, 'max_ms': 2, 'status_counts': {'200': 1, '503': 1}}
            with contextlib.redirect_stdout(io.StringIO()):
                scope['main']()
            result = json.loads((root / 'out.json').read_text())
            self.assertEqual(len(result['routes']), 3)
            self.assertIn('/api/tabs/existing-tab/chat', result['routes'])
            self.assertEqual(result['routes']['/api/tabs']['status_counts'], {'200': 1, '503': 1})
unittest.main()
`;
  const result = spawnSync('python', ['-c', checks], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
