import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from native import export, cancelled, export_to_file


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def file(self, name, rows):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(''.join(json.dumps(r) + '\n' for r in rows))
        return str(path)

    def test_large_native_export_file_preserves_unicode_and_exact_record(self):
        text = 'λ界' * 40000
        path = self.file('large-own.jsonl', [
            {'sessionId': 'own', 'type': 'user', 'message': {'content': text}}])
        spec = {'provider': 'claude', 'session_id': 'own', 'path': path}
        transfer = export_to_file(spec)
        destination = Path(transfer['path'])
        try:
            payload = destination.read_bytes()
            self.assertGreater(len(payload), 65536)
            self.assertEqual(len(payload), transfer['bytes'])
            self.assertEqual(json.loads(payload)['records'][0]['text'], text)
            self.assertEqual(json.loads(payload)['session_id'], 'own')
            self.assertEqual(__import__('hashlib').sha256(payload).hexdigest(), transfer['sha256'])
        finally:
            destination.unlink()
            destination.parent.rmdir()

    def test_claude_excludes_foreign_and_sidechain(self):
        path = self.file('own.jsonl', [
            {'sessionId': 'own', 'type': 'user', 'message': {'content': 'token'}},
            {'sessionId': 'foreign', 'type': 'assistant', 'message': {'content': 'bad', 'model': 'expensive'}},
            {'sessionId': 'own', 'isSidechain': True, 'type': 'assistant', 'message': {'content': 'side'}},
            {'sessionId': 'own', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'answer'}], 'model': 'haiku'}}])
        result = export({'provider': 'claude', 'session_id': 'own', 'path': path})
        self.assertEqual([r['text'] for r in result['records']], ['token', 'answer'])
        self.assertEqual(result['records'][-1]['model'], 'haiku')
        self.assertEqual(result['records'][-1]['locator'], 'line:4')
        self.assertTrue(result['sources'][0]['content'])

    def test_codex_rollout_binding_and_turn_model(self):
        path = self.file('rollout.jsonl', [
            {'type': 'session_meta', 'payload': {'id': 'own'}},
            {'type': 'turn_context', 'payload': {'model': 'gpt-6-luna'}},
            {'type': 'response_item', 'payload': {'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'answer'}]}}])
        result = export({'provider': 'codex', 'session_id': 'own', 'path': path})
        self.assertEqual(result['records'][-1]['model'], 'gpt-6-luna')
        with self.assertRaises(ValueError):
            export({'provider': 'codex', 'session_id': 'foreign', 'path': path})

    def test_grok_summary_exact_directory_binding(self):
        path = self.file('own/updates.jsonl', [
            {'params': {'update': {'sessionUpdate': 'user_message_chunk', 'content': {'text': 'token'}, '_meta': {'modelId': 'grok-4.6'}}}},
            {'params': {'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'text': 'answer'}}}}])
        summary = self.root / 'own/summary.json'
        summary.write_text(json.dumps({'info': {'id': 'own'}, 'current_model_id': 'grok-4.6'}))
        spec = {'provider': 'grok', 'session_id': 'own', 'path': path, 'summary_path': str(summary)}
        result = export(spec)
        self.assertEqual(result['records'][-1]['model'], 'grok-4.6')
        summary.write_text(json.dumps({'info': {'id': 'foreign'}}))
        with self.assertRaises(ValueError):
            export(spec)

    def test_opencode_queries_only_exact_session(self):
        path = self.root / 'records.db'
        connection = sqlite3.connect(path)
        connection.executescript('CREATE TABLE session(id TEXT); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,session_id TEXT,message_id TEXT,time_created INTEGER,data TEXT);')
        connection.execute('INSERT INTO session VALUES (?)', ('own',))
        for sid, text in [('own', 'answer'), ('foreign', 'bad')]:
            connection.execute('INSERT INTO message VALUES(?,?,?,?)', (sid, sid, 1, json.dumps({'role': 'assistant', 'providerID': 'deepseek', 'modelID': 'deepseek-flash'})))
            connection.execute('INSERT INTO part VALUES(?,?,?,?,?)', (sid, sid, sid, 1, json.dumps({'type': 'text', 'text': text})))
        connection.commit()
        connection.close()
        result = export({'provider': 'opencode', 'session_id': 'own', 'path': str(path)})
        self.assertEqual([r['text'] for r in result['records']], ['answer'])
        self.assertEqual(result['records'][0]['model'], 'deepseek/deepseek-flash')
        self.assertEqual(result['records'][0]['raw']['session_id'], 'own')

    def test_cancel_words_in_assistant_prose_are_not_cancellation(self):
        self.assertFalse(cancelled('codex', {'type': 'response_item', 'payload': {'role': 'assistant', 'content': 'cancel'}}))
        self.assertTrue(cancelled('codex', {'type': 'event_msg', 'payload': {'type': 'turn_aborted'}}))

    def test_malformed_json_is_a_failure(self):
        path = self.root / 'bad.jsonl'
        path.write_text('{broken')
        with self.assertRaises(ValueError):
            export({'provider': 'claude', 'session_id': 'own', 'path': str(path)})


if __name__ == '__main__':
    unittest.main()
