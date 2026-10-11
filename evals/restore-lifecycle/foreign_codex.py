"""Run from the Worker tool environment, preserving its CODETERM_* identity."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile


def main():
    marker = sys.argv[1]
    required = ('CODETERM_TAB_ID', 'CODETERM_SESSION_BINDING_NONCE', 'CODETERM_ATTEMPT_ID')
    missing = [key for key in required if not os.environ.get(key)]
    if missing:
        raise SystemExit('missing inherited Worker identity: ' + ', '.join(missing))
    with tempfile.TemporaryDirectory(prefix='rl-foreign-') as cwd:
        prompt = pathlib.Path(cwd) / 'prompt.md'
        prompt.write_text(f'Reply with exactly {marker}. Do not run tools or commands.')
        with prompt.open() as source:
            result = subprocess.run(['codex', 'exec', '--skip-git-repo-check', '--json', '-'],
                                    stdin=source, cwd=cwd, capture_output=True, text=True, timeout=90)
        events = []
        for line in result.stdout.splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        sessions = [row.get('thread_id') for row in events if row.get('type') == 'thread.started']
        own = os.environ.get('CODETERM_SESSION_ID')
        if result.returncode or len(sessions) != 1 or sessions[0] == own:
            raise SystemExit(json.dumps({'foreign_exit': result.returncode, 'sessions': sessions, 'stderr': result.stderr}))
        print(json.dumps({'marker': marker, 'foreign_exit': result.returncode, 'foreign_session': sessions[0],
                          'inherited_tab': os.environ['CODETERM_TAB_ID'],
                          'inherited_nonce': os.environ['CODETERM_SESSION_BINDING_NONCE'],
                          'inherited_attempt': os.environ['CODETERM_ATTEMPT_ID']}))


if __name__ == '__main__':
    main()
