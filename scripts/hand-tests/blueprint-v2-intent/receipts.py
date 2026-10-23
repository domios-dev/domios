import hashlib
import json
import os
from pathlib import Path
import re
import sys
import time
import uuid


def digest(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def redact(text):
    text = re.sub(r'(?i)(Bearer\s+)[A-Za-z0-9._~+/-]+', r'\1[REDACTED]', text)
    text = re.sub(r'(?i)("(?:api_token|api_key|access_token|refresh_token)"\s*:\s*")[^"]+', r'\1[REDACTED]', text)
    return re.sub(r'\b(?:sk-ant-|sk-proj-)[A-Za-z0-9_-]+', '[REDACTED]', text)


def save(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(redact(json.dumps(value, indent=2)) + '\n')


def walk(value):
    yield value
    if isinstance(value, dict):
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def cli_receipt(context_path, capture, code, argv):
    context = json.loads(Path(context_path).read_text())
    directory = Path(context['receipts'])
    directory.mkdir(parents=True, exist_ok=True)
    prefix = directory / f'{time.time_ns()}-{uuid.uuid4().hex}'
    stdout = Path(capture, 'stdout').read_text(errors='replace')
    stderr = Path(capture, 'stderr').read_text(errors='replace')
    prefix.with_suffix('.stdout').write_text(redact(stdout))
    prefix.with_suffix('.stderr').write_text(redact(stderr))
    save(prefix.with_suffix('.json'), {'argv': argv, 'exit': int(code), 'tab': context['tab'],
                                      'instance': 'blueprint-v2-proof', 'port': 17693,
                                      'stdout_sha256': digest(stdout), 'stderr_sha256': digest(stderr)})


if __name__ == '__main__':
    if sys.argv[1] != 'cli':
        raise SystemExit('unknown receipt operation')
    cli_receipt(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5:])
