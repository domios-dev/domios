import unittest

import codex_oracle as oracle
import opencode_evidence as evidence


class OpenCodeEvidenceTests(unittest.TestCase):
    def test_restore_roster_excludes_cheap_provider_without_losing_worker_or_plain_spawn(self):
        cases = evidence.spawn_cases('default', 'cheap', default_only=True)
        self.assertEqual({model for _, model, _ in cases}, {'default'})
        self.assertEqual({worker for _, _, worker in cases}, {True, False})
        self.assertEqual(len(cases), 2)
        self.assertIn(('Epsilon', 'cheap', True), evidence.spawn_cases('default', 'cheap'))

    def test_startup_prompt_composes_both_launch_roles_with_opaque_marker(self):
        marker = 'opaque-round-trip-token'
        for worker in (True, False):
            with self.subTest(worker=worker):
                prompt = evidence.startup_prompt(worker, marker)
                self.assertIsInstance(prompt, str)
                self.assertEqual(prompt.count(marker), 1)

    def row(self, key, kind, content):
        return dict(id=key, type=kind, content=content)

    def test_lazy_session_restore_requires_own_provider_and_session(self):
        row = dict(agent_type='opencode', agent_session_id='own', session_binding_nonce='n',
                   reported_session_binding_nonce='n', boot_stage='agent_ready', structured_control_active=False)
        oracle.bindings({'a': row}, {'a': row}, provider='opencode')
        with self.assertRaisesRegex(AssertionError, 'changed'):
            oracle.bindings({'a': {**row, 'agent_session_id': 'foreign'}}, {'a': row}, provider='opencode')

    def test_chat_ids_reject_duplicates_and_missing(self):
        for rows in ([self.row('a', 'user', 'x')] * 2, [self.row(None, 'user', 'x')]):
            with self.assertRaisesRegex(AssertionError, 'duplicate'):
                evidence.row_ids(rows)

    def test_restore_cannot_hide_missing_or_changed_user_rows(self):
        before = [self.row('a', 'user', 'opaque')]
        evidence.retained(before, before + [self.row('b', 'assistant', 'reply')])
        with self.assertRaisesRegex(AssertionError, 'lost'):
            evidence.retained(before, [])
        with self.assertRaisesRegex(AssertionError, 'changed'):
            evidence.retained(before, [self.row('a', 'user', 'replacement')])

    def test_idle_zero_and_working_one_ignore_assistant_echo(self):
        old = [self.row('old', 'user', oracle.CONTINUE)]
        evidence.continues(old, old, 0)
        new = old + [self.row('echo', 'assistant', oracle.CONTINUE)]
        with self.assertRaisesRegex(AssertionError, 'continues=0'):
            evidence.continues(old, new, 1)
        new += [self.row('new', 'user', oracle.CONTINUE)]
        evidence.continues(old, new, 1)
        with self.assertRaisesRegex(AssertionError, 'owed=0'):
            evidence.continues(old, new, 0)
        new += [self.row('duplicate', 'user', oracle.CONTINUE)]
        with self.assertRaisesRegex(AssertionError, 'continues=2'):
            evidence.continues(old, new, 1)

    def test_resume_requires_numeric_original_work_after_continue(self):
        rows = [self.row('a', 'assistant', '1\n2'), self.row('b', 'user', oracle.CONTINUE),
                self.row('c', 'assistant', 'ack')]
        self.assertFalse(evidence.resumed(rows))
        rows.append(self.row('d', 'assistant', '3\n4'))
        self.assertTrue(evidence.resumed(rows))

    def test_repeated_transcript_text_records_same_role_rows_only(self):
        rows = [self.row('a', 'user', 'opaque'), self.row('b', 'assistant', 'opaque'),
                self.row('c', 'assistant', 'opaque'), self.row('d', 'tool_call', '')]
        self.assertEqual(evidence.repeated_text(rows), [dict(type='assistant', content='opaque', count=2)])

    def test_timings_ignore_other_tabs_and_old_launches(self):
        log = '\n'.join(('[restart-timing] tab=a stage=first-composer pty=p at_ms=900',
                         '[restart-timing] tab=b stage=first-composer pty=p at_ms=1020',
                         '[restart-timing] tab=a stage=first-composer pty=p at_ms=1050',
                         '[restart-timing] tab=a stage=first-composer pty=p at_ms=1080'))
        self.assertEqual(evidence.timing(log, 'a', 1000), {'first-composer': 50})


if __name__ == '__main__':
    unittest.main()
