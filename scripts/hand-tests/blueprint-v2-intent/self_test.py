import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from oracles import checked_definition, verify_authored, verify_cards, verify_identity, verify_run
from receipts import digest
from subjects import decode_codex, subject_environment

HERE = Path(__file__).resolve().parent
PIN = {'id': 'mem:fixture', 'hash': 'a' * 64}


class OracleTests(unittest.TestCase):
    def codex_stream(self, warning=None, late=False):
        events = [{'type': 'thread.started', 'thread_id': 'fresh-session'}]
        item = {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'finished'}}
        error = {'type': 'item.completed', 'item': {'type': 'error', 'message': warning}}
        events += ([item, error] if late else [error, item]) if warning else [item]
        events.append({'type': 'turn.completed', 'usage': {'input_tokens': 100, 'cached_input_tokens': 30,
                       'cache_write_input_tokens': 10, 'output_tokens': 20, 'reasoning_output_tokens': 7}})
        return '\n'.join(map(json.dumps, events))

    def test_erratum_ignores_only_the_early_exact_warning_and_never_doubles_reasoning(self):
        data = decode_codex(self.codex_stream('Under-development features enabled: flags'))
        self.assertEqual(data['usage'], {'input_tokens': 60, 'cache_read_input_tokens': 30,
                         'cache_creation_input_tokens': 10, 'output_tokens': 20, 'reasoning_output_tokens': 7})
        for warning, late in [('other failure', False), ('Under-development features enabled: flags', True)]:
            with self.assertRaises(ValueError):
                decode_codex(self.codex_stream(warning, late))

    def test_scrubbed_env_never_inherits_prod_selectors_or_secrets(self):
        original = dict(os.environ)
        try:
            os.environ.update(CODETERM_API_TOKEN='fake-secret', CODETERM_API_PORT='7685', CODETERM_TASK_ID='foreign')
            env = subject_environment(Path('/temporary/proxy'), ['/usr/bin/model'])
            self.assertEqual(sorted(k for k in env if k.startswith('CODETERM')), ['CODETERM_DEV_API_PORT', 'CODETERM_DEV_INSTANCE'])
            self.assertNotIn('fake-secret', env.values())
        finally:
            os.environ.clear()
            os.environ.update(original)

    def test_sealed_definition_requires_a_real_successful_check(self):
        self.assertEqual(checked_definition({'ok': True, 'definition': {'name': 'opaque'}}), {'name': 'opaque'})
        for data in [{'ok': False}, {'ok': True}, {'ok': True, 'definition': {}, 'diagnostics': [{'severity': 'error'}]}]:
            with self.assertRaises(ValueError):
                checked_definition(data)

    def test_identify_must_prove_the_selected_qa_port_before_launch(self):
        connection = {'instance': 'blueprint-v2-proof', 'host': '127.0.0.1', 'port': 17693}
        verify_identity({'connection': connection})
        for changed in [{'port': 7685}, {'port': 7686}, {'instance': ''}, {'host': 'remote'}]:
            with self.assertRaises(ValueError):
                verify_identity({'connection': {**connection, **changed}})

    def definition(self):
        return {'schema_version': 2, 'kind': 'pipeline', 'name': 'case', 'dependencies': {'mem:fixture@' + PIN['hash']: {}},
                'steps': [{'id': 'invoke', 'call': {'ref': PIN['id'], 'pin': PIN, 'with': {'code': {'lit': 7}}}},
                          {'id': 'sink', 'run': {'lit': 'fixed sink'}, 'in': {'type': 'integer'}, 'bind': {'in': 'invoke.out'}}]}

    def test_false_no_fit_reuse_and_fabricated_pins_fail(self):
        valid = self.definition()
        verify_authored(valid, 'case', PIN, 'fixed sink')
        with self.assertRaises(ValueError):
            verify_authored(valid, 'case', None, 'fixed sink')
        for field in ['hash', 'id']:
            invalid = self.definition()
            invalid['steps'][0]['call']['pin'] = {**PIN, field: 'invented'}
            with self.assertRaises(ValueError):
                verify_authored(invalid, 'case', PIN, 'fixed sink')
        invalid = self.definition()
        invalid['dependencies'] = {}
        with self.assertRaises(ValueError):
            verify_authored(invalid, 'case', PIN, 'fixed sink')

    def test_full_body_card_leak_and_missing_candidate_are_refused(self):
        card = {**PIN, 'provenance': 'mem:', 'score': 1.0, 'when_to_use': 'purpose', 'why': 'vector',
                'signature': {'inputs': {}, 'result': {'type': 'integer'}}, 'node_kinds': {'code': 1}, 'enrichment_pending': False}
        value = {'cards': [card], 'scaffold': {'schema_version': 2}, 'recall': {'status': 'ready'}}
        self.assertEqual(verify_cards(value, PIN), card)
        card['code'] = 'body'
        with self.assertRaises(ValueError):
            verify_cards(value, PIN)
        with self.assertRaises(ValueError):
            verify_cards({'cards': [], 'scaffold': {'schema_version': 2}}, PIN)

    def test_actual_sink_envelope_and_effect_are_required(self):
        with tempfile.TemporaryDirectory() as root:
            effect = Path(root) / 'effect.txt'
            envelope = {'media': 'json', 'schema_id': None, 'value': 73, 'text': ''}
            result = subprocess.run([sys.executable, '-I', str(HERE / 'sink.py'), str(effect)],
                                    input=json.dumps(envelope), text=True, capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            log = [{'step_name': '$run', 'state': 'done'}, {'state': 'done', 'result_summary': 'code completed'}]
            verify_run({}, log, effect, 73)
            with self.assertRaises(ValueError):
                verify_run({}, log, effect, 11)
            with self.assertRaises(ValueError):
                verify_run({}, [{'step_name': '$run', 'state': 'done'}], effect, 73)
            self.assertNotIn('73', (HERE / 'sink.py').read_text())

    def test_prompt_decision_is_the_accepted_bytes_and_keeps_the_generic_example(self):
        decision = json.loads((HERE / 'prompt-decision.json').read_text())
        prompt = (HERE.parents[2] / decision['product_path']).read_text()
        self.assertEqual(decision['decision'], 'PASS')
        self.assertTrue(decision['audit_pass'])
        self.assertEqual(decision['product_sha256'], digest(prompt))
        self.assertEqual(decision['source_sha256'], 'bf77023a394f71dfce8bc1bbc0ec5b47c5b8f04002e7714905a71c3fa6d708a9')
        tasks = json.loads((HERE / 'tasks.json').read_text())
        positive = tasks['positive'].format(name='trial', sink='fixed sink')
        self.assertNotIn('73', positive)


if __name__ == '__main__':
    unittest.main(verbosity=2)
