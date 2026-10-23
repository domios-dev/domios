import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import path from 'node:path';

const bash = process.platform === 'win32' ? path.join(process.env.ProgramFiles, 'Git', 'bin', 'bash.exe') : 'bash';
const python = process.platform === 'win32' ? 'python' : 'python3';

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
    def test_worker_times_direct_http_without_identity_checks(self):
        import subprocess
        worker = '''CODETERM_API_URL=http://127.0.0.1:17686
CODETERM_API_TOKEN=opaque-fixture
dev_get() { exit 99; }
curl() {
[[ "$*" == *"--max-time 30"* && "$*" == *"Authorization: Bearer opaque-fixture"* ]] || exit 98
case "\u0024{!#}" in
  http://127.0.0.1:17686/ok) printf '0.0022 200\\n' ;;
  http://127.0.0.1:17686/bad) printf '0.0030 503\\n' ;;
  http://127.0.0.1:17686/down) printf '0.0010 000\\n'; return 7 ;;
  *) exit 97 ;;
esac
}
''' + scope['WORKER']
        result = subprocess.run([${JSON.stringify(bash)}, '-c', worker], input='/ok\\n/bad\\n/down\\n', capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['0.0022 200', '0.0030 503', '0.0010 000'])
    def test_measure_uses_http_duration_and_preserves_status(self):
        from unittest.mock import MagicMock, patch
        scope['sys'].argv = ['fixture', '1', 'out', 'tab', 'log', 'bash']
        for status, expected in [('200', '200'), ('503', '503'), ('000', 'transport_error')]:
            worker = MagicMock()
            worker.__enter__.return_value = worker
            worker.stdout.readline.return_value = '0.0022 ' + status
            with patch.object(scope['subprocess'], 'Popen', return_value=worker), \\
                 patch.object(scope['time'], 'monotonic', side_effect=[0, 0, 2, 2, 2, 2]), \\
                 patch.object(scope['threading'], 'Event'):
                result = scope['measure']('/opaque', 1)
            self.assertEqual(result['samples'], 1)
            self.assertAlmostEqual(result['p50_ms'], 2.2)
            self.assertEqual(result['status_counts'], {expected: 1})
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
  const result = spawnSync(python, ['-c', checks], { encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `signal=${result.signal}\n${result.stdout}${result.stderr}`);
});

test('lock storm reads the daemon log and validates DEV before direct sampling', () => {
  const setup = build.split('  qa-lock-storm)')[1].split("<<'LOCK_STORM_PY'")[0];
  assert.match(setup, /storm_log="\$\(dev_data_dir\)\/logs\/domios\.log"/);
  assert.match(setup, /dev_resolve\n\s+storm_tabs="\$\(dev_get \/api\/tabs\)"/);
  assert.match(setup, /dev_run python3/);
  assert.doesNotMatch(source, /source evals\/|dev_get/);
});
