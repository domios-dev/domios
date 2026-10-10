import argparse
import hashlib
import itertools
import json
from pathlib import Path

PROVIDERS = ('shell', 'claude', 'codex', 'opencode', 'grok')
CHECKS = ('output', 'resize-reflow', 'scrollback', 'colors', 'cursor', 'alternate-enter', 'alternate-exit', 'input-ctrl-c')
VIEWS = ('local', 'remote', 'web')
DIRECTIONS = ('mac-home', 'windows-home')


def matrix(providers=PROVIDERS):
    return [dict(zip(('provider', 'check', 'view', 'direction'), key))
            for key in itertools.product(providers, CHECKS, VIEWS, DIRECTIONS)]


def coordinate(row):
    return tuple(row[k] for k in ('provider', 'check', 'view', 'direction'))


def validate(root, row, providers=PROVIDERS):
    if coordinate(row) not in {coordinate(r) for r in matrix(providers)}:
        raise ValueError('unknown matrix coordinate')
    if row['status'] not in ('PASS', 'FAIL'):
        raise ValueError('observations require PASS or FAIL')
    if not row.get('tab_id') or not row.get('observation'):
        raise ValueError('tab and observed result required')
    required = ['home_before', 'home_after', 'view_before', 'view_after',
                'home_formatted', 'view_formatted', 'renderer_state', 'event_evidence']
    if row['provider'] != 'shell':
        required += ['provider_record', 'binding']
        if not row.get('session_id') or not row.get('provider_record_lines'):
            raise ValueError('exact session and persisted record line references required')
    if row['status'] == 'FAIL':
        required += ['repro', 'logs']
    artifacts = {}
    for key in required:
        rel = Path(row['evidence'][key])
        path = (root / rel).resolve()
        if rel.is_absolute() or not path.is_relative_to(root.resolve()):
            raise ValueError('evidence must remain inside run directory')
        data = path.read_bytes()
        if not data:
            raise ValueError('empty evidence: ' + key)
        artifacts[key] = {'path': str(rel), 'sha256': hashlib.sha256(data).hexdigest()}
    return artifacts


def render(root, providers):
    observations = [json.loads(p.read_text()) for p in sorted((root / 'observations').glob('*.json'))]
    lines = ['# Phase B3 terminal fidelity', '',
             'PENDING is unexecuted coverage, never a PASS. Every observation is retained; any FAIL keeps its cell FAIL.', '',
             '| Provider | Check | View | Home | Result | Evidence |',
             '|---|---|---|---|---|---|']
    for cell in matrix(providers):
        found = [r for r in observations if coordinate(r) == coordinate(cell)]
        result = 'FAIL' if any(r['status'] == 'FAIL' for r in found) else ('PASS' if found else 'PENDING')
        links = ', '.join('[%s](observations/%s.json)' % (r['attempt'], r['attempt']) for r in found) or 'not executed'
        lines.append('| %s | %s | %s | %s | %s | %s |' % (*coordinate(cell), result, links))
    (root / 'results.md').write_text('\n'.join(lines) + '\n')


def record(root, row):
    providers = tuple(json.loads((root / 'manifest.json').read_text())['providers'])
    attempt = row['attempt']
    if not attempt or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_' for c in attempt):
        raise ValueError('unsafe observation id')
    row['artifacts'] = validate(root, row, providers)
    with (root / 'observations' / (attempt + '.json')).open('x') as out:
        json.dump(row, out, indent=2)
    render(root, providers)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('init', 'record'))
    parser.add_argument('root', type=Path)
    parser.add_argument('--observation', type=Path)
    parser.add_argument('--mimo', action='store_true')
    args = parser.parse_args()
    if args.action == 'init':
        args.root.mkdir(parents=True, exist_ok=False)
        (args.root / 'observations').mkdir()
        providers = PROVIDERS + (('mimo',) if args.mimo else ())
        (args.root / 'manifest.json').write_text(json.dumps({'providers': providers, 'cells': matrix(providers)}, indent=2))
        render(args.root, providers)
    else:
        record(args.root, json.loads(args.observation.read_text()))


if __name__ == '__main__':
    main()
