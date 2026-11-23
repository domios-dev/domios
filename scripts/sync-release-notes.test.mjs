import assert from 'node:assert/strict';
import test from 'node:test';
import { canonicalPullRequestBody, changelogNotes, chooseCanonicalBody } from './sync-release-notes.mjs';

const changelog = `# Changelog

## [Unreleased]

- Not shipped yet.

## [1.22.0]

### Highlights

- Live transcription in chat.

### Fixed

- Mobile toggle keeps its height.

### Known issues

- Dictation needs a microphone grant.

## [1.21.1]

### Fixed

- Older fix.
`;

test('changelog notes take exactly the version section under the public title heading', () => {
  const notes = changelogNotes('v1.22.0', changelog);
  assert.match(notes, /^## Domios 1\.22\.0\n\n### Highlights/);
  assert.match(notes, /### Known issues\n\n- Dictation needs a microphone grant\.$/);
  assert.doesNotMatch(notes, /Older fix|Not shipped/);
  assert.equal(changelogNotes('v1.23.0', changelog), '');
});

test('release PR fallback keeps user-facing changes and drops operator evidence and metadata', () => {
  const body = canonicalPullRequestBody('v1.21.2', [
    '## Domios 1.21.2', '', 'This release uses commit `711e54cdb35a7067ac74592cc9e2e9102b592c49`.', '',
    '- Recover a stale PTY after a launch deadline.', 'Repository-Issue: domios-dev/domios#12',
    'Release-Test-Waiver: {"run_id":"1","jobs":[]}', '',
    '## Verification', '', '- Exact-SHA CI passed.', '',
    '## Known issues', '', '- Dictation needs a microphone grant.', '',
    '## Owner publication approval', '', 'The owner selected Publish.',
  ].join('\n'));
  assert.match(body, /^## Domios 1\.21\.2\n\n- Recover a stale PTY/);
  assert.match(body, /## Known issues\n\n- Dictation needs a microphone grant\.$/);
  assert.doesNotMatch(body, /Exact-SHA|owner selected|Repository-Issue|Release-Test-Waiver|uses commit/);
});

test('placeholder qualification drafts take the replacement notes', () => {
  const marker = '<!-- qualification:{"source_sha":"a","run_id":"1"} -->';
  const notes = changelogNotes('v1.22.0', changelog);
  assert.equal(chooseCanonicalBody('v1.22.0', marker, notes), notes);
});
