import json
from pathlib import Path
import re

from receipts import walk


def verify_identity(value):
    connection = value.get('connection', {})
    if (connection.get('instance') != 'blueprint-v2-proof'
            or str(connection.get('port')) != '17693'
            or connection.get('host') not in ['127.0.0.1', '::1', 'localhost']):
        raise ValueError('scrubbed subject identify resolved outside the selected QA instance/port')


def checked_definition(value):
    if value.get('ok') is not True or not isinstance(value.get('definition'), dict):
        raise ValueError('real check did not return an admitted definition')
    if any(d.get('severity') == 'error' for d in value.get('diagnostics', [])):
        raise ValueError('check contains an error diagnostic')
    return value['definition']


def calls(value):
    return [item['call'] for item in walk(value) if isinstance(item, dict) and isinstance(item.get('call'), dict)]


def verify_authored(definition, name, selected, sink):
    if definition.get('schema_version') != 2 or definition.get('kind') != 'pipeline' or definition.get('name') != name:
        raise ValueError('wrong native graph identity')
    found = calls(definition)
    if selected:
        matches = [call for call in found if call.get('ref') == selected['id']]
        if len(matches) != 1 or len(found) != 1:
            raise ValueError('author did not select exactly the saved operation')
        pin = matches[0].get('pin', {})
        if pin != {'id': selected['id'], 'hash': selected['hash']}:
            raise ValueError('call did not retain the exact discovered id/hash')
        if not definition.get('dependencies'):
            raise ValueError('pinned call has no immutable callee snapshot')
        if matches[0].get('with') != {'code': {'lit': 7}}:
            raise ValueError('call arguments differ from the typed stored signature')
        if any(isinstance(item, dict) and 'code' in item for item in definition.get('steps', [])):
            raise ValueError('positive case copied a body instead of calling it')
    elif found or definition.get('dependencies'):
        raise ValueError('no-fit case falsely reused a call')
    fixed = [node for node in definition.get('steps', []) if node.get('run') == {'lit': sink}]
    if len(fixed) != 1 or fixed[0].get('in') != {'type': 'integer'} or not fixed[0].get('bind', {}).get('in'):
        raise ValueError('fixed sink did not consume the typed returned stdin value')
    return definition


def verify_cards(value, selected=None):
    if not isinstance(value.get('cards'), list) or value.get('scaffold', {}).get('schema_version') != 2:
        raise ValueError('new --intent did not return cards and a native scaffold')
    found = None
    for card in value['cards']:
        required = {'id', 'hash', 'provenance', 'score', 'when_to_use', 'why', 'signature', 'node_kinds', 'enrichment_pending'}
        if not required.issubset(card) or set(card) & {'yaml', 'document', 'code', 'steps', 'body'}:
            raise ValueError('card lacks typed metadata or leaks a body')
        if not isinstance(card['score'], (float, int)) or not re.fullmatch('[a-f0-9]{64}', card['hash']):
            raise ValueError('invalid real score/content hash')
        if '\n' in card['when_to_use'] or '\n' in card['why']:
            raise ValueError('card purpose/WHY is not one line')
        if selected and card['id'] == selected['id']:
            if card['enrichment_pending'] or value.get('recall', {}).get('status') != 'ready':
                raise ValueError('saved solution vector/enrichment readiness is pending')
            if re.search(r'(?<![.\d])73(?!\d)', card['when_to_use'] + ' ' + card['why']):
                raise ValueError('card reveals the hidden expected result')
            found = card
    if selected and found is None:
        raise ValueError('saved operation absent from actual returned candidates')
    return found


def verify_run(show, log, effect, expected, selected=None):
    if not Path(effect).is_file() or Path(effect).read_text().strip() != str(expected):
        raise ValueError(f'missing/wrong actual sink integer for {Path(effect).name}')
    envelope = json.loads(Path(effect).with_suffix('.envelope.json').read_text())
    if envelope.get('value') != expected or type(envelope.get('value')) is not int:
        raise ValueError('sink envelope is not the actual typed integer')
    entries = log if isinstance(log, list) else log.get('entries', log.get('data', []))
    if not any(e.get('step_name') == '$run' and e.get('state') == 'done' for e in entries):
        raise ValueError('real run has no completed retained run record')
    executed = [e for e in entries if str(e.get('result_summary', '')).endswith('code completed') and e.get('state') == 'done']
    if not executed:
        raise ValueError('no retained executed code-node evidence')
    if selected:
        objects = [item for item in walk(show) if isinstance(item, dict)]
        if not any(item.get('id') == selected['id'] and item.get('hash') == selected['hash'] for item in objects):
            raise ValueError('show lost the executed callee id/hash')
        if not any('steps' in item and any(isinstance(n, dict) and 'code' in n for n in item['steps']) for item in objects if isinstance(item.get('steps'), list)):
            raise ValueError('show does not expose the expanded callee')
