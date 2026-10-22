import json
from pathlib import Path
import sys

body = sys.stdin.buffer.read(32769)
if len(body) > 32768:
    raise SystemExit('stdin exceeds envelope bound')
envelope = json.loads(body)
if set(envelope) != {'media', 'schema_id', 'value', 'text'}:
    raise SystemExit('missing native stdin envelope')
if envelope['media'] != 'json' or type(envelope['value']) is not int or envelope['text'] != '':
    raise SystemExit('expected a typed integer JSON envelope')
output = Path(sys.argv[1])
with output.open('x') as stream:
    stream.write(str(envelope['value']) + '\n')
with output.with_suffix('.envelope.json').open('x') as stream:
    json.dump(envelope, stream)
print(json.dumps(envelope))
