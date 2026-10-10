from pathlib import Path
import re
import unittest


SETUP_COMPLETE = '<!-- bootstrap:setup-complete -->'
POST_SETUP = '<!-- bootstrap:post-setup-supporter -->'
GUIDE_DIRECTIVE = '@ds-guide supporter-agent'
PROMPT = Path(__file__).resolve().parents[2] / 'src-tauri/prompts/bootstrap_instructions.md'


class BootstrapStructureTests(unittest.TestCase):
    def test_fragment_follows_last_setup_action(self):
        source = PROMPT.read_text(encoding='utf-8')
        self.assertEqual(source.count(SETUP_COMPLETE), 1)
        self.assertEqual(source.count(POST_SETUP), 1)
        setup_start = source.index('### Step 4:')
        setup_end = source.index('### Step 5:')
        setup = source[setup_start:setup_end]
        actions = list(re.finditer(r'^(\d+)\. ', setup, re.MULTILINE))
        self.assertEqual([int(action[1]) for action in actions], [1, 2, 3, 4, 5])
        self.assertLess(actions[-1].start(), setup.index(SETUP_COMPLETE))
        self.assertLess(source.index(SETUP_COMPLETE), source.index('### Step 5:'))
        self.assertLess(source.index('### Step 5:'), source.index(POST_SETUP))

    def test_guide_control_is_inside_the_post_setup_fragment(self):
        source = PROMPT.read_text(encoding='utf-8')
        before, marker, fragment = source.partition(POST_SETUP)
        self.assertTrue(marker)
        self.assertNotIn(GUIDE_DIRECTIVE, before)
        self.assertEqual(fragment.splitlines().count(GUIDE_DIRECTIVE), 1)


if __name__ == '__main__':
    unittest.main()
