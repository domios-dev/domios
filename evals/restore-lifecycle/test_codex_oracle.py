import json
import os
import pathlib
import subprocess
import tempfile
import unittest

import codex_oracle as oracle


class EvidenceTests(unittest.TestCase):
    def test_cli_dispatch_envelope_and_direct_payload(self):
        dispatch = {'tab_id': 'own', 'state': 'awaiting_worker_ack'}
        self.assertEqual(oracle.cli_data({'ok': True, 'kind': 'task', 'data': dispatch}), dispatch)
        self.assertEqual(oracle.cli_data(dispatch), dispatch)
        self.assertEqual(oracle.cli_data([dispatch]), [dispatch])
        with self.assertRaisesRegex(AssertionError, 'refused'):
            oracle.cli_data({'ok': False, 'kind': 'error', 'data': None})
        with self.assertRaisesRegex(AssertionError, 'refused'):
            oracle.cli_data({'ok': False, 'error': {'code': 'tab_busy_refused', 'bytes_written': False}})

    def test_rollout_requires_exact_embedded_session_identity(self):
        with tempfile.TemporaryDirectory() as root:
            path = pathlib.Path(root) / 'rollout-own.jsonl'
            path.write_text(json.dumps({'type': 'session_meta', 'payload': {'id': 'foreign'}}))
            with self.assertRaisesRegex(AssertionError, 'expected one'):
                oracle.own_rollout(root, 'own')
            path.write_text(json.dumps({'type': 'session_meta', 'payload': {'id': 'own'}}))
            self.assertEqual(oracle.own_rollout(root, 'own'), path)

    def row(self, sid='own'):
        return dict(agent_type='codex', agent_session_id=sid, session_binding_nonce='n',
                    reported_session_binding_nonce='n', boot_stage='agent_ready', structured_control_active=True)

    def test_duplicate_and_foreign_nonce_refused(self):
        with self.assertRaisesRegex(AssertionError, 'duplicate'):
            oracle.bindings({'a': self.row(), 'b': self.row()})
        row = self.row()
        row['reported_session_binding_nonce'] = 'foreign'
        with self.assertRaisesRegex(AssertionError, 'nonce'):
            oracle.bindings({'a': row})

    def test_restored_session_must_stay_owned(self):
        oracle.bindings({'a': self.row()}, {'a': self.row()})
        with self.assertRaisesRegex(AssertionError, 'changed'):
            oracle.bindings({'a': self.row('foreign')}, {'a': self.row()})

    def test_readiness_not_implied_by_session(self):
        row = self.row()
        row['boot_stage'] = 'provider_ready'
        with self.assertRaisesRegex(AssertionError, 'boot_stage'):
            oracle.bindings({'a': row})

    def test_user_turns_exclude_event_echo_and_assistant_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'rollout.jsonl'
            def message(role, text):
                return dict(type='response_item', payload=dict(type='message', role=role, content=[dict(text=text)]))
            rows = [dict(type='event_msg', payload=dict(type='user_message', message='marker')),
                    message('user', 'marker'), message('assistant', 'marker')]
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            self.assertEqual(oracle.user_texts(path), ['marker'])
            self.assertEqual(oracle.assistant_texts(path), ['marker'])

    def test_resumed_output_must_follow_the_new_continue(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'rollout.jsonl'
            def message(role, text):
                return dict(type='response_item', payload=dict(type='message', role=role, content=[dict(text=text)]))
            rows = [message('user', 'original'), message('assistant', '1\n2'),
                    message('user', oracle.CONTINUE), message('assistant', 'queued marker')]
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            self.assertFalse(oracle.resumed_numeric_output(path, 1))
            rows.append(message('assistant', '3\n4'))
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            self.assertTrue(oracle.resumed_numeric_output(path, 1))
            self.assertFalse(oracle.resumed_numeric_output(path, 2))

    def test_tool_evidence_excludes_agent_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'rollout.jsonl'
            rows = [dict(type='response_item', payload=dict(type='message', role='assistant', content=[dict(text='claimed')]))]
            rows.append(dict(type='response_item', payload=dict(type='function_call_output', output='actual')))
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            rows.append(dict(type='response_item', payload=dict(type='custom_tool_call_output', output=[dict(type='input_text', text='array output')])))
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            self.assertEqual(oracle.tool_outputs(path), ['actual', 'array output'])

    def test_marker_duplicate_and_cross_delivery_refused(self):
        oracle.exact_markers({'a': ['opaque'], 'b': []}, {'a': 'opaque'})
        for texts in ({'a': ['opaque', 'opaque'], 'b': []}, {'a': ['opaque'], 'b': ['opaque']}):
            with self.assertRaisesRegex(AssertionError, 'count='):
                oracle.exact_markers(texts, {'a': 'opaque'})

    def test_restart_requires_one_delivered_outcome_per_tab(self):
        log = '[turn-resume] tab=a session=s outcome=delivered evidence=write_handoff'
        oracle.restart_logs(log, ['a'])
        with self.assertRaisesRegex(AssertionError, 'outcomes=2'):
            oracle.restart_logs(log + '\n' + log, ['a'])
        with self.assertRaisesRegex(AssertionError, 'outcomes=0'):
            oracle.restart_logs('', ['a'])
        with self.assertRaisesRegex(AssertionError, 'expired_launch'):
            oracle.restart_logs(log + '\nqueue outcome=expired_launch', ['a'])

    def test_idle_requires_zero_continues(self):
        oracle.restart_logs('', ['a'], idle=True)
        with self.assertRaisesRegex(AssertionError, 'outcomes=1'):
            oracle.restart_logs('[turn-resume] tab=a outcome=delivered', ['a'], idle=True)

    def test_daemon_environment_requires_actual_minimal_path(self):
        env = 'PATH=' + oracle.GUI_PATH + ' CODETERM_INSTANCE=restartqa CODETERM_API_PORT=7692'
        oracle.daemon_environment(env, 'gui')
        with self.assertRaisesRegex(AssertionError, 'daemon PATH'):
            oracle.daemon_environment(env.replace(oracle.GUI_PATH, '/opt/homebrew/bin:' + oracle.GUI_PATH), 'gui')
        with self.assertRaisesRegex(AssertionError, '7692'):
            oracle.daemon_environment(env.replace('7692', '7686'), 'gui')

    def test_closed_attempt_cannot_pass_respawn(self):
        attempt = dict(id='new', executor_tab_id='b', acknowledged_at=1)
        self.assertEqual(oracle.current_attempt({'task': {'execution_attempts': [attempt]}}, 'b')['id'], 'new')
        attempt['acknowledged_at'] = None
        with self.assertRaisesRegex(AssertionError, 'ACK missing'):
            oracle.current_attempt({'execution_attempts': [attempt]}, 'b')
        attempt['acknowledged_at'] = 1
        attempt['closed_at'] = 2
        with self.assertRaisesRegex(AssertionError, 'open attempt'):
            oracle.current_attempt({'execution_attempts': [attempt]}, 'b')

    def test_observer_uses_dev_run_url_and_refuses_prod(self):
        root = pathlib.Path(__file__).parent
        env = {**os.environ, 'PYTHONPATH': str(root), 'CODETERM_API_URL': 'http://127.0.0.1:7692'}
        result = subprocess.run(['python3', '-c', 'import observe; print(observe.BASE)'], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), env['CODETERM_API_URL'])
        env['CODETERM_API_URL'] = 'http://127.0.0.1:7685'
        result = subprocess.run(['python3', '-c', 'import observe'], env=env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
