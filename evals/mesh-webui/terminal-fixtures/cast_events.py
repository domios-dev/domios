import json


def parse_cast(text):
    lines = text.splitlines()
    header = json.loads(lines[0])
    if header.get('version') != 2 or header.get('width', 0) < 1 or header.get('height', 0) < 1:
        raise ValueError('invalid asciicast v2 header')
    events = []
    previous = 0
    for line in lines[1:]:
        event = json.loads(line)
        if len(event) != 3:
            raise ValueError('invalid event shape')
        at, kind, data = event
        if not isinstance(at, (int, float)) or at < previous or kind not in ('o', 'i', 'r') or not isinstance(data, str):
            raise ValueError('invalid event sequence')
        previous = at
        events.append({'at': at, 'kind': kind, 'data': data})
    return header, events
