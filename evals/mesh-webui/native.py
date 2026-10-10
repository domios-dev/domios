import base64
import hashlib
import json
import sqlite3
import sys
import tempfile
from pathlib import Path

LIMIT = 16 * 1024 * 1024


def read(path):
    path = Path(path).expanduser().resolve(strict=True)
    data = path.read_bytes()
    if len(data) > LIMIT:
        raise ValueError('Native record exceeds fixed 16MiB bound')
    return str(path), data


def blocks(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return ''.join(x.get('text', '') for x in content if isinstance(x, dict))
    if isinstance(content, dict):
        return content.get('text', '')
    return ''


def jsonl(path):
    source, data = read(path)
    return source, data, [(i, json.loads(line)) for i, line in enumerate(data.decode().splitlines(), 1) if line.strip()]


def cancelled(provider, raw):
    if provider == 'codex':
        return raw.get('type') == 'event_msg' and raw.get('payload', {}).get('type') == 'turn_aborted'
    if provider == 'claude':
        content = raw.get('message', {}).get('content')
        return raw.get('type') == 'user' and blocks(content) in (
            '[Request interrupted by user]', '[Request interrupted by user for tool use]')
    if provider == 'opencode':
        message = json.loads(raw['data'])
        return message.get('error', {}).get('name') == 'MessageAbortedError'
    if provider == 'grok':
        update = raw.get('params', {}).get('update', {})
        return update.get('sessionUpdate') == 'prompt_response' and update.get('stopReason') == 'cancelled'
    return False


def export(spec):
    provider, sid = spec['provider'], spec['session_id']
    if not sid:
        raise ValueError('Exact claimed session ID required')
    records, sources = [], []
    session_model = None

    def add(raw, source, locator, role='', text='', model=None, tool=False, attachment=False):
        records.append(dict(raw=raw, source_path=source, locator=locator, role=role,
                            text=text, model=model, tool=tool, attachment=attachment, cancelled=cancelled(provider, raw)))

    def source_file(path):
        name, data, rows = jsonl(path)
        sources.append(dict(path=name, sha256=hashlib.sha256(data).hexdigest(), content=data.decode()))
        return name, rows

    if provider == 'claude':
        source, rows = source_file(spec['path'])
        if not any(raw.get('sessionId') == sid for _, raw in rows):
            raise ValueError('Claude file lacks exact sessionId')
        for line, raw in rows:
            if raw.get('sessionId') != sid or raw.get('isSidechain') is True:
                continue
            msg = raw.get('message', {})
            content = msg.get('content', raw.get('content', ''))
            add(raw, source, 'line:' + str(line), raw.get('type', ''), blocks(content), msg.get('model'),
                any(b.get('type') == 'tool_use' for b in content if isinstance(b, dict)) if isinstance(content, list) else False,
                any(b.get('type') in ('image', 'document') for b in content if isinstance(b, dict)) if isinstance(content, list) else False)
    elif provider == 'codex':
        source, rows = source_file(spec['path'])
        metas = [raw['payload'] for _, raw in rows if raw.get('type') == 'session_meta']
        if len(metas) != 1 or metas[0].get('id') != sid:
            raise ValueError('Codex rollout exact session_meta binding required')
        model = None
        for line, raw in rows:
            payload = raw.get('payload', {})
            if raw.get('type') == 'turn_context':
                model = payload.get('model')
            role = payload.get('role', '') if raw.get('type') == 'response_item' else ''
            add(raw, source, 'line:' + str(line), role, blocks(payload.get('content')), model,
                payload.get('type') in ('function_call', 'custom_tool_call'),
                any(c.get('type') == 'input_image' for c in payload.get('content', []) if isinstance(c, dict)))
    elif provider == 'grok':
        summary_path, summary_bytes = read(spec['summary_path'])
        summary = json.loads(summary_bytes)
        if summary.get('info', {}).get('id') != sid:
            raise ValueError('Grok summary belongs to another session')
        path = Path(spec['path']).expanduser().resolve(strict=True)
        if path.parent != Path(summary_path).parent or path.parent.name != sid or path.name != 'updates.jsonl':
            raise ValueError('Grok updates must share exact summary session directory')
        session_model = summary.get('current_model_id')
        sources.append(dict(path=summary_path, sha256=hashlib.sha256(summary_bytes).hexdigest(), content=summary_bytes.decode()))
        source, rows = source_file(path)
        model = None
        for line, raw in rows:
            update = raw.get('params', {}).get('update', {})
            kind = update.get('sessionUpdate')
            if kind == 'user_message_chunk':
                model = update.get('_meta', {}).get('modelId')
            role = {'user_message_chunk': 'user', 'agent_message_chunk': 'assistant'}.get(kind, '')
            add(raw, source, 'line:' + str(line), role, blocks(update.get('content')), model,
                kind == 'tool_call', update.get('content', {}).get('type') in ('image', 'resource') if isinstance(update.get('content'), dict) else False)
    elif provider == 'opencode':
        path = Path(spec['path']).expanduser().resolve(strict=True)
        connection = sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=5)
        connection.row_factory = sqlite3.Row
        try:
            connection.execute('BEGIN')
            session = connection.execute('SELECT * FROM session WHERE id = ?', (sid,)).fetchone()
            if session is None:
                raise ValueError('OpenCode exact session absent')
            sources.append(dict(path=str(path), table='session', row=dict(session)))
            rows = connection.execute('SELECT id, session_id, time_created, data FROM message WHERE session_id = ? ORDER BY time_created, id', (sid,)).fetchall()
            size = sum(len(r['data']) for r in rows)
            if size > LIMIT:
                raise ValueError('OpenCode messages exceed fixed bound')
            for row in rows:
                raw = dict(row)
                msg = json.loads(row['data'])
                parts = [dict(p) for p in connection.execute('SELECT id, session_id, message_id, data FROM part WHERE session_id = ? AND message_id = ? ORDER BY time_created, id', (sid, row['id']))]
                raw['parts'] = parts
                size += sum(len(p['data']) for p in parts)
                if size > LIMIT:
                    raise ValueError('OpenCode parts exceed fixed bound')
                parsed = [json.loads(p['data']) for p in parts]
                model = f"{msg['providerID']}/{msg['modelID']}" if msg.get('providerID') and msg.get('modelID') else None
                add(raw, str(path), 'message.id:' + row['id'], msg.get('role', ''),
                    ''.join(p.get('text', '') for p in parsed if p.get('type') == 'text'), model,
                    any(p.get('type') == 'tool' for p in parsed), any(p.get('type') == 'file' for p in parsed))
            connection.rollback()
        finally:
            connection.close()
    else:
        raise ValueError('Unsupported provider')
    return dict(provider=provider, session_id=sid, records=records, sources=sources, session_model=session_model)


def export_to_file(spec):
    payload = json.dumps(export(spec), ensure_ascii=False).encode('utf-8')
    if len(payload) > LIMIT:
        raise ValueError('Native export exceeds fixed 16MiB transfer bound')
    directory = Path(tempfile.mkdtemp(prefix='phaseb-native-'))
    destination = directory / 'native.json'
    destination.write_bytes(payload)
    return dict(path=str(destination), bytes=len(payload),
                sha256=hashlib.sha256(payload).hexdigest())


if __name__ == '__main__':
    spec = json.loads(base64.b64decode(sys.argv[1]))
    print(json.dumps(export(spec), ensure_ascii=False))
