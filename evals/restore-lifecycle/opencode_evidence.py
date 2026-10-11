"""Pure checks over tab-owned OpenCode Chat rows and launch timing logs."""
import re

from codex_oracle import CONTINUE, require


def texts(rows, kind):
    return [row.get('content', '') for row in rows if row.get('type') == kind]


def row_ids(rows):
    ids = [row.get('id') for row in rows]
    require(all(ids) and len(ids) == len(set(ids)), 'missing or duplicate Chat row ids')
    return ids


def retained(before, after):
    old = {row['id']: row for row in before}
    new = {row['id']: row for row in after}
    require(old.keys() <= new.keys(), 'transcript rows lost during restart')
    for key, row in old.items():
        if row.get('type') == 'user':
            require(new[key].get('content') == row.get('content'), f'user row changed: {key}')


def continues(before, after, owed):
    old = set(row_ids(before))
    delta = [row for row in after if row['id'] not in old]
    count = sum(CONTINUE in text for text in texts(delta, 'user'))
    require(count == owed, f'owed={owed}, own-session continues={count}')
    if not owed:
        require(texts(before, 'user') == texts(after, 'user'), 'idle restart sent or lost a user turn')
    return delta


def timing(log, tab, start_ms):
    matches = re.findall(r'\[restart-timing\] tab=' + re.escape(tab) + r' stage=([^ ]+) pty=[^ ]+ at_ms=(\d+)', log)
    values = {}
    for stage, timestamp in matches:
        milliseconds = int(timestamp) - start_ms
        if milliseconds >= 0:
            values.setdefault(stage, milliseconds)
    return values


def resumed(rows):
    seen_continue = False
    for row in rows:
        if row.get('type') == 'user' and CONTINUE in row.get('content', ''):
            seen_continue = True
        if seen_continue and row.get('type') == 'assistant' and any(line.strip().isdigit() for line in row.get('content', '').splitlines()):
            return True
    return False


def repeated_text(rows):
    counts = {}
    for row in rows:
        kind, content = row.get('type'), row.get('content', '')
        if kind in ('user', 'assistant') and content.strip():
            key = (kind, content)
            counts[key] = counts.get(key, 0) + 1
    return [dict(type=kind, content=content, count=count)
            for (kind, content), count in counts.items() if count > 1]


def spawn_cases(default, cheap, default_only=False):
    return [('Delta', default, True)] + ([] if default_only else [('Epsilon', cheap, True)]) + [('Zeta', default, False)]
