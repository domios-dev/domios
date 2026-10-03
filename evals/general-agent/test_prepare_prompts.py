import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock


PREPARER_PATH = Path(__file__).with_name('prepare-prompts.py')
SPEC = importlib.util.spec_from_file_location('prepare_prompts', PREPARER_PATH)
PREPARER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARER)


class PreparedPromptTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        prompts = self.root / 'src-tauri/prompts'
        prompts.mkdir(parents=True)
        self.baseline = {}
        names = [
            'general_agent.md', 'codeterm_rule.md', 'bridge_mem.md',
            'bootstrap_instructions.md', 'agent_codeterm_primer.md',
            'agent_pad_rule.md', 'rich_rendering_guidance.md',
            'codeterm_cli.md', 'interactive_child_pty.md',
        ]
        for index, name in enumerate(names):
            old = f'old-fragment-{index}'
            new = f'new-fragment-{index}'
            if name == 'general_agent.md':
                suffix = '\n{codeterm_cli}\n{interactive_child_pty_operation}'
                old += suffix
                new += suffix
            self.baseline[f'src-tauri/prompts/{name}'] = old
            (prompts / name).write_text(new, encoding='utf-8')
        fixture_dir = self.root / 'evals/general-agent'
        fixture_dir.mkdir(parents=True)
        self.evals = [
            {'id': 1, 'prompt_family': 'general-agent'},
            {'id': 2, 'prompt_family': 'bootstrap'},
        ]
        (fixture_dir / 'evals.json').write_text(json.dumps({'evals': self.evals}))
        self.output = self.root / 'prepared'

    def prepare(self):
        args = ['prepare-prompts.py', '--baseline-ref', 'opaque-ref',
                '--output-dir', str(self.output)]
        with mock.patch.object(PREPARER, '__file__', str(self.root / 'evals/general-agent/prepare-prompts.py')):
            with mock.patch.object(PREPARER, 'at_revision', side_effect=lambda root, ref, path: self.baseline[path]):
                with mock.patch('sys.argv', args), contextlib.redirect_stdout(io.StringIO()):
                    PREPARER.main()

    def test_current_general_and_memory_fragments_reach_candidate(self):
        self.prepare()
        for family in ['general-agent', 'bootstrap']:
            value = (self.output / f'{family}-new.md').read_text()
            self.assertIn('new-fragment-0', value)
            self.assertIn('new-fragment-1', value)
            self.assertIn('new-fragment-2', value)
            self.assertNotIn('old-fragment-', value)

    def test_cli_pad_and_recovery_substitutions_reach_both_arms(self):
        self.prepare()
        for arm, prefix in [('old', 'old'), ('new', 'new')]:
            value = (self.output / f'general-agent-{arm}.md').read_text()
            for index in [4, 5, 7, 8]:
                self.assertEqual(value.count(f'{prefix}-fragment-{index}'), 1)
            self.assertNotIn('{codeterm_cli}', value)
            self.assertNotIn('{interactive_child_pty_operation}', value)
            self.assertLess(value.index(f'{prefix}-fragment-4'), value.index(f'{prefix}-fragment-0'))

    def test_candidate_pad_change_is_not_replaced_by_baseline(self):
        self.prepare()
        value = (self.output / 'spawned-agent-new.md').read_text()
        self.assertEqual(value.count('new-fragment-5'), 1)
        self.assertNotIn('old-fragment-', value)

    def test_first_run_fragment_and_fixture_stay_in_their_family(self):
        self.prepare()
        for arm, prefix in [('old', 'old'), ('new', 'new')]:
            self.assertNotIn(f'{prefix}-fragment-3', (self.output / f'general-agent-{arm}.md').read_text())
            self.assertEqual((self.output / f'bootstrap-{arm}.md').read_text().count(f'{prefix}-fragment-3'), 1)
        self.assertEqual(json.loads((self.output / 'general-agent-evals.json').read_text())['evals'], self.evals[:1])
        self.assertEqual(json.loads((self.output / 'bootstrap-evals.json').read_text())['evals'], self.evals[1:])


if __name__ == '__main__':
    unittest.main()
