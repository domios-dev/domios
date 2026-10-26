import assert from 'node:assert/strict';
import test from 'node:test';
import { pageName, renderWiki } from '../wiki-sync.mjs';

const essences = {
  alpha: '# Alpha: opaque one\n\nSee [beta](beta.md#beta:part) and [gone](missing.md).\n\n## Part {#alpha:part}\n\n```\n# not a title {#not:an-id}\n[raw](beta.md)\n```\n',
  beta: '# Beta\n\n### Part {#beta:part}\n\nBody.\n',
  vision: '# Vision opaque\n\nText.\n',
};

test('page names keep words and drop punctuation', () => {
  assert.equal(pageName('Begin here: documentation contract'), 'Begin-here-documentation-contract');
});

test('essences render as pages with rewritten links, kept anchors and untouched code', () => {
  const files = renderWiki(essences, 'fixture/repo');
  const alpha = files['Alpha-opaque-one.md'];
  assert.match(alpha, /^> Generated from \[essences\/alpha\.md\]\(https:\/\/github\.com\/fixture\/repo\/blob\/master\/essences\/alpha\.md\)/);
  assert.doesNotMatch(alpha, /^# Alpha/m);
  assert.match(alpha, /\]\(Beta#beta:part\)/);
  assert.match(alpha, /\]\(missing\.md\)/);
  assert.match(alpha, /<a name="alpha:part"><\/a>\n\n## Part\n/);
  assert.match(alpha, /# not a title \{#not:an-id\}\n\[raw\]\(beta\.md\)/);
  assert.match(files['Beta.md'], /<a name="beta:part"><\/a>\n\n### Part\n/);
});

test('home and sidebar list known essences first, then the rest by name', () => {
  const files = renderWiki(essences, 'fixture/repo');
  const order = files['_Sidebar.md'].match(/\]\(([^)]+)\)/g);
  assert.deepEqual(order, ['](Home)', '](Vision-opaque)', '](Alpha-opaque-one)', '](Beta)']);
  assert.match(files['Home.md'], /- \[Vision opaque\]\(Vision-opaque\)/);
});
