import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

test('compiler diagnostics distinguish owned directory boundaries and foreign source paths', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'lane-foreign-'));
  try {
    mkdirSync(path.join(root, 'src-tauri', 'crates', 'foreign'), { recursive: true });
    writeFileSync(path.join(root, 'src-tauri', 'crates', 'foreign', 'lib.rs'), 'fixture');
    const script = path.resolve(import.meta.dirname, '../lane-check-run.py');
    const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-c', `
import io, json, runpy, sys
module = runpy.run_path(sys.argv[1])
diagnostic = {'spans': [{'file_name': 'crates/foreign/lib.rs', 'is_primary': True}]}
paths = module['diagnostic_paths'](diagnostic, sys.argv[2])
assert paths == ['src-tauri/crates/foreign/lib.rs'], paths
assert module['foreign_paths'](paths, ['src-tauri/crates/own']) == paths
assert module['foreign_paths'](paths, ['src-tauri/crates/foreign']) == []
assert module['foreign_paths'](['src/owned-other.rs'], ['src/owned']) == ['src/owned-other.rs']
lookup = lambda args: [{'id': 'worker', 'role': 'Worker', 'task_binding': {'task_id': 'task-a', 'attempt_id': 'attempt-a'}}]
assert module['diagnostic_binding']({'CODETERM_PARENT_TAB_ID': 'worker'}, lookup) == ('task-a', 'attempt-a')
assert module['diagnostic_binding']({'CODETERM_TASK_ID': 'own-task', 'CODETERM_ATTEMPT_ID': 'own-attempt'}, lookup) == ('own-task', 'own-attempt')
assert module['diagnostic_binding']({'CODETERM_PARENT_TAB_ID': 'foreign'}, lookup) is None
assert module['diagnostic_binding']({}, lookup) is None
def read(args):
    if args[0] == 'tab': return lookup(args)
    return {'task': {'id': 'task-a', 'files': {'edit': ['src-tauri/crates/own']}, 'execution_attempts': [{'id': 'attempt-a', 'placement': 'shared'}]}}
output = io.StringIO()
module['report_foreign_errors']([diagnostic], env={'CODETERM_PARENT_TAB_ID': 'worker'}, read=read, checkout=sys.argv[2], output=output)
assert 'foreign compile failure in src-tauri/crates/foreign/lib.rs' in output.getvalue(), output.getvalue()
print('foreign diagnostic classification passed')
`, script, root], { encoding: 'utf8' });
    assert.equal(result.status, 0, `${result.error?.message ?? ''}\n${result.stderr}`);
    assert.match(result.stdout, /classification passed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('raw CLI decoding requires one exact tab and a valid current task; diagnostics preserve Cargo exit', () => {
  const script = path.resolve(import.meta.dirname, '../lane-check-run.py');
  const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-c', `
import copy, io, json, os, runpy, subprocess, sys
from types import SimpleNamespace
from unittest.mock import patch
module = runpy.run_path(sys.argv[1])
read = module['read_cli']
worker = {'id': 'worker', 'role': 'Worker', 'task_binding': {'task_id': 'task-a', 'attempt_id': 'attempt-a'}}
task = {'task': {'id': 'task-a', 'files': {'edit': ['src/own']}, 'execution_attempts': [{'id': 'attempt-a', 'placement': 'shared'}]}}
def check_tab(value, expected):
    def cli(args, **options):
        assert args == ['domios', 'tab', 'inspect', '--tab', 'worker', '--json'], args
        assert options['check'] and options['timeout'] == 10
        return SimpleNamespace(stdout=json.dumps(value))
    with patch.object(subprocess, 'run', cli):
        assert module['diagnostic_binding']({'CODETERM_PARENT_TAB_ID': 'worker'}, read) == expected, value
check_tab([worker], ('task-a', 'attempt-a'))
for value in [[], {}, {'result': [worker]}, {'data': [worker]}, [None], [worker, worker],
              [{**worker, 'id': 'foreign'}], [{**worker, 'tab_id': 'foreign'}], [worker, {**worker, 'id': 'foreign'}],
              [{**worker, 'role': 'General'}], [{**worker, 'task_binding': []}],
              [{**worker, 'task_binding': {'task_id': '', 'attempt_id': 'attempt-a'}}]]:
    check_tab(value, None)
assert module['diagnostic_binding']({'CODETERM_TASK_ID': 'task-a'}, lambda _: (_ for _ in ()).throw(AssertionError('no fallback'))) is None
def task_cli(args, **options):
    assert args == ['domios', 'task', 'show', 'task-a', '--json'], args
    return SimpleNamespace(stdout=json.dumps(task))
with patch.object(subprocess, 'run', task_cli):
    assert module['shared_attempt_task'](read(['task', 'show', 'task-a']), 'task-a', 'attempt-a') == task['task']
for value in [None, [], {**task, 'ok': False}, {**task, 'error': 'opaque'}, {'result': task}, {'task': None}, {'task': {**task['task'], 'id': 'foreign'}},
              {'task': {**task['task'], 'files': {'edit': 'src/own'}}},
              {'task': {**task['task'], 'execution_attempts': [None]}},
              {'task': {**task['task'], 'execution_attempts': task['task']['execution_attempts'] * 2}}]:
    assert module['shared_attempt_task'](value, 'task-a', 'attempt-a') is None, value
for changes in [{'id': 'foreign'}, {'closed_at': 0}, {'retired_at': 1}, {'placement': 'lane'}]:
    value = copy.deepcopy(task)
    value['task']['execution_attempts'][0].update(changes)
    assert module['shared_attempt_task'](value, 'task-a', 'attempt-a') is None
class Cargo:
    stdout = iter([json.dumps({'reason': 'compiler-message', 'message': {'level': 'error', 'rendered': 'opaque E0425', 'spans': []}})])
    def __enter__(self): return self
    def __exit__(self, *args): return False
    def wait(self): return 101
for observed in ['[', 'null', '{"task": []}', '[]']:
    cargo = Cargo()
    cargo.stdout = iter([json.dumps({'reason': 'compiler-message', 'message': {'level': 'error', 'rendered': 'opaque E0425', 'spans': []}})])
    with (
        patch.dict(os.environ, {'CODETERM_TASK_ID': 'task-a', 'CODETERM_ATTEMPT_ID': 'attempt-a'}, clear=True),
        patch.object(subprocess, 'Popen', lambda _: cargo),
        patch.object(subprocess, 'run', lambda *args, **kwargs: SimpleNamespace(stdout=observed)),
    ):
        assert module['run_cargo'](['opaque-cargo']) == 101, observed
for failure in [OSError('unavailable'), subprocess.TimeoutExpired('domios', 10), TypeError('invalid observation')]:
    output = io.StringIO()
    with patch.object(subprocess, 'run', side_effect=failure):
        module['report_foreign_errors']([{'spans': []}], env={'CODETERM_PARENT_TAB_ID': 'worker'}, read=read, output=output)
    assert 'failure remains unclassified' in output.getvalue()
print('raw CLI shape and Cargo exit conservation passed')
`, script], { encoding: 'utf8' });
  assert.equal(result.status, 0, `${result.error?.message ?? ''}\n${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /Cargo exit conservation passed/);
});
