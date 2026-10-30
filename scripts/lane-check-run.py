import contextlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys


def diagnostic_paths(message, checkout):
    paths = []
    for span in message.get('spans', []):
        if not span.get('is_primary'):
            continue
        source = Path(span['file_name'])
        candidates = [source] if source.is_absolute() else [Path(checkout) / source, Path(checkout) / 'src-tauri' / source, Path(checkout) / 'cli' / source]
        for candidate in candidates:
            if candidate.is_file():
                try:
                    paths.append(candidate.resolve().relative_to(Path(checkout).resolve()).as_posix())
                except ValueError:
                    pass
                break
    return paths


def foreign_paths(paths, edits):
    normalize = lambda value: os.path.normcase(value.replace('\\', '/')).replace('\\', '/').rstrip('/')
    owned = [normalize(edit) for edit in edits]
    return [source for source in paths if not any(normalize(source) == edit or normalize(source).startswith(edit + '/') for edit in owned)]


def read_cli(args):
    result = subprocess.run(['domios', *args, '--json'], capture_output=True, text=True, timeout=10, check=True)
    return json.loads(result.stdout)


def nonempty_id(value):
    return isinstance(value, str) and bool(value.strip())


def diagnostic_binding(env, inspect):
    task_id = env.get('CODETERM_TASK_ID')
    attempt_id = env.get('CODETERM_ATTEMPT_ID')
    if task_id or attempt_id:
        return (task_id, attempt_id) if nonempty_id(task_id) and nonempty_id(attempt_id) else None
    parent = env.get('CODETERM_PARENT_TAB_ID')
    if not parent or env.get('CODETERM_ROLE'):
        return None
    rows = inspect(['tab', 'inspect', '--tab', parent])
    if not isinstance(rows, list) or len(rows) != 1:
        return None
    worker = rows[0]
    if not isinstance(worker, dict) or worker.get('id') != parent or worker.get('tab_id', parent) != parent or str(worker.get('role') or '').lower() != 'worker':
        return None
    binding = worker.get('task_binding') or {}
    if isinstance(binding, dict) and nonempty_id(binding.get('task_id')) and nonempty_id(binding.get('attempt_id')):
        return binding['task_id'], binding['attempt_id']
    return None


def shared_attempt_task(value, task_id, attempt_id):
    if not isinstance(value, dict) or value.get('ok') is False or value.get('error') is not None or not isinstance(value.get('task'), dict):
        return None
    task = value['task']
    attempts = task.get('execution_attempts')
    files = task.get('files')
    if task.get('id') != task_id or not isinstance(attempts, list) or not isinstance(files, dict):
        return None
    edits = files.get('edit')
    if not isinstance(edits, list) or not all(nonempty_id(edit) for edit in edits):
        return None
    if not all(isinstance(attempt, dict) and nonempty_id(attempt.get('id')) for attempt in attempts):
        return None
    matches = [attempt for attempt in attempts if attempt['id'] == attempt_id]
    if len(matches) != 1:
        return None
    attempt = matches[0]
    if attempt.get('closed_at') is not None or attempt.get('retired_at') is not None or attempt.get('placement') != 'shared':
        return None
    return task


def report_foreign_errors(errors, env=None, read=read_cli, checkout=None, output=sys.stderr):
    if not errors:
        return
    try:
        binding = diagnostic_binding(os.environ if env is None else env, read)
        if not binding:
            return
        task_id, attempt_id = binding
        task = shared_attempt_task(read(['task', 'show', task_id]), task_id, attempt_id)
        if task is None:
            return
        paths = [source for error in errors for source in diagnostic_paths(error, checkout or os.getcwd())]
        for source in sorted(set(foreign_paths(paths, task.get('files', {}).get('edit', [])))):
            print(f'lane-check: foreign compile failure in {source}; report to the Orchestrator, do not edit outside ownership', file=output)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print('lane-check: compile ownership unavailable; failure remains unclassified', file=output)


def run_cargo(command):
    errors = []
    with subprocess.Popen(command, stdout=subprocess.PIPE, text=True, encoding='utf-8', errors='replace') as cargo:
        for line in cargo.stdout:
            try:
                record = json.loads(line)
            except ValueError:
                print(line, end='', flush=True)
                continue
            if record.get('reason') == 'compiler-message':
                message = record['message']
                print(message.get('rendered', ''), end='', flush=True)
                if message.get('level') == 'error':
                    errors.append(message)
        code = cargo.wait()
    if code:
        report_foreign_errors(errors)
    return code


def run(directory, command):
    root = Path(directory)
    root.mkdir(parents=True, exist_ok=True)
    lock = root / '.domios-lane-check.lock'
    with lock.open('ab'):
        pass
    cargo_lock = runpy.run_path(str(Path(__file__).with_name('cargo-build-gc.py')))['cargo_lock']
    with contextlib.ExitStack() as stack:
        try:
            queued = os.environ.get('CODETERM_BUILD_GATEWAY_MODE') == 'queue'
            if queued:
                print(f'lane-check queued: waiting for shared intermediates: {root}', flush=True)
            stack.enter_context(cargo_lock(lock, blocking=queued))
        except OSError as error:
            if error.errno in (11, 13) or getattr(error, 'winerror', None) in (32, 33):
                print(f'lane-check busy: shared intermediates are owned by another check: {root}', file=sys.stderr)
                return 75
            raise
        return run_cargo(command)


if __name__ == '__main__':
    sys.exit(run(sys.argv[1], sys.argv[2:]))
