import argparse
import hashlib
import itertools
import json
from pathlib import Path

PROVIDERS = ('claude', 'codex', 'opencode', 'grok')
DIRECTIONS = ('mac-web-to-windows', 'windows-web-to-mac')
VIEWS = ('local', 'remote', 'web UI')
CHECKS = ('peer-switch', 'gateway-return', 'tab-list', 'chat-render', 'composer-send',
          'stop', 'model-switch', 'managed-by', 'status-working', 'status-done',
          'progress-live', 'sync-within-1s', 'tool-rows', 'code-blocks', 'attachments',
          'history-reload', 'medium-chat-15-30')


def keys():
    return itertools.product(PROVIDERS, CHECKS, VIEWS, DIRECTIONS)


def pointer(record, path):
    if path == '':
        return record
    if not path.startswith('/'):
        raise ValueError('JSON pointer must start with /')
    for key in path[1:].split('/'):
        key = key.replace('~1', '/').replace('~0', '~')
        record = record[int(key)] if isinstance(record, list) else record[key]
    return record


def select_records(raw, session_pointer, session_id):
    if not session_id:
        raise ValueError('Exact claimed session required')
    return [r for r in raw if pointer(r, session_pointer) == session_id]


def parse_jsonl(contents):
    return [json.loads(line) for line in contents.splitlines() if line.strip()]


def save_evidence(root, contents, suffix):
    digest = hashlib.sha256(contents).hexdigest()
    target = root / 'evidence' / (digest + suffix)
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        if target.read_bytes() != contents:
            raise ValueError('Evidence hash collision')
    else:
        with target.open('xb') as stream:
            stream.write(contents)
    return str(target.relative_to(root))


def checked_path(root, relative):
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError('Evidence must exist within this run')
    return path


def validate(root, row):
    key = tuple(row[k] for k in ('provider', 'check', 'view', 'direction'))
    if key not in set(keys()):
        raise ValueError('Unknown matrix cell')
    if row['status'] not in ('PASS', 'FAIL'):
        raise ValueError('Executed receipt must be PASS or FAIL')
    for name in ('evidence', 'screenshot'):
        checked_path(root, row[name])
    if not row.get('tab_id') or not row.get('session_id'):
        raise ValueError('Tab and exact session IDs required')
    if row['status'] == 'FAIL':
        for name in ('steps', 'log_lines', 'expected', 'observed'):
            if not row.get(name):
                raise ValueError('FAIL requires exact repro: ' + name)
    if row['status'] == 'PASS' and row['check'] in (
            'composer-send', 'model-switch', 'medium-chat-15-30'):
        proof = row['provider_record']
        raw = json.loads(checked_path(root, proof['path']).read_text())
        if pointer(raw, proof['session_pointer']) != row['session_id']:
            raise ValueError('Provider record belongs to another session')
        if pointer(raw, proof['claim_pointer']) != proof['expected_value']:
            raise ValueError('Provider record does not prove claimed turn/model')
        if not proof.get('source_path') or not proof.get('source_locator'):
            raise ValueError('Provider persisted source path and line/row locator required')
    return key


def render(root, receipts):
    grouped = {}
    for row in receipts:
        key = validate(root, row)
        grouped.setdefault(key, []).append(row)
    lines = ['# Phase B2 evidence matrix', '',
             'NOT RUN is missing execution, never PASS or a product FAIL.', '',
             '| Provider | Check | View | Direction | Verdict | Evidence |',
             '|---|---|---|---|---|---|']
    for key in keys():
        rows = grouped.get(key, [])
        if not rows:
            lines.append('| ' + ' | '.join((*key, 'NOT RUN', 'No live receipt')) + ' |')
        else:
            for row in rows:
                lines.append('| ' + ' | '.join((*key, row['status'],
                             '[' + row['evidence'] + '](' + row['evidence'] + ')')) + ' |')
    root.mkdir(parents=True, exist_ok=True)
    (root / 'results.md').write_text('\n'.join(lines) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--receipts', type=Path)
    args = parser.parse_args()
    rows = json.loads(args.receipts.read_text()) if args.receipts else []
    render(args.out, rows)
