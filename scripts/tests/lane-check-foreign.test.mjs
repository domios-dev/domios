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
lookup = lambda args: {'id': 'worker', 'role': 'Worker', 'task_binding': {'task_id': 'task-a', 'attempt_id': 'attempt-a'}}
assert module['diagnostic_binding']({'CODETERM_PARENT_TAB_ID': 'worker'}, lookup) == ('task-a', 'attempt-a')
assert module['diagnostic_binding']({'CODETERM_TASK_ID': 'own-task', 'CODETERM_ATTEMPT_ID': 'own-attempt'}, lookup) == ('own-task', 'own-attempt')
assert module['diagnostic_binding']({'CODETERM_PARENT_TAB_ID': 'foreign'}, lookup) is None
assert module['diagnostic_binding']({}, lookup) is None
def read(args):
    if args[0] == 'tab': return lookup(args)
    return {'task': {'files': {'edit': ['src-tauri/crates/own']}, 'execution_attempts': [{'id': 'attempt-a', 'placement': 'shared'}]}}
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
