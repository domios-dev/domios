import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../../src-tauri/src/startup/frontend_shell/initial_document.js', import.meta.url), 'utf8');

function documentFixture({ readyState = 'loading', markWidth = 76, status = 'Opaque status', role = 'status', hasMark = true } = {}) {
  const frames = [], events = new Map(), styles = [];
  const main = {
    querySelector(selector) {
      if (selector === 'svg') return hasMark ? { getBoundingClientRect: () => ({ width: markWidth }) } : null;
      if (selector === 'h1') return { textContent: status, getBoundingClientRect: () => ({ height: 24 }) };
      return null;
    },
  };
  const window = {}, location = {};
  runInNewContext(source, {
    window, location,
    document: {
      readyState,
      head: { append: style => styles.push(style) },
      createElement: () => ({}),
      addEventListener: (name, handler, options) => {
        assert.equal(options.once, true);
        events.set(name, handler);
      },
      querySelector: selector => selector === 'main[role]' && role ? main : null,
    },
    requestAnimationFrame: callback => frames.push(callback),
  });
  return { window, location, frames, styles, events };
}

for (const role of ['status', 'alert']) {
  test(`Starting ${role} waits for content and a paint opportunity before acknowledgement`, () => {
    const fixture = documentFixture({ role });
    assert.equal(fixture.styles.length, 0);
    fixture.events.get('DOMContentLoaded')();
    assert.equal(fixture.styles.length, 1);
    fixture.window.__DOMIOS_STARTING_PAINT__(7);
    assert.equal(fixture.location.href, undefined);
    assert.equal(fixture.frames.length, 1);
    fixture.frames.shift()();
    assert.equal(fixture.location.href, undefined);
    assert.equal(fixture.frames.length, 1);
    fixture.frames.shift()();
    assert.equal(fixture.location.href, 'domios-starting-painted://localhost/7');
    fixture.window.__DOMIOS_STARTING_PAINT__(7);
    assert.equal(fixture.frames.length, 0);
  });
}

test('An empty or unlaid-out Starting document cannot acknowledge paint', () => {
  for (const options of [{ status: '' }, { status: '  ' }, { hasMark: false }, { role: null }, { markWidth: 0 }]) {
    const fixture = documentFixture({ ...options, readyState: 'complete' });
    assert.equal(fixture.styles.length, 1);
    fixture.window.__DOMIOS_STARTING_PAINT__(3);
    for (const frame of fixture.frames.splice(0)) frame();
    assert.equal(fixture.frames.length, 0);
    assert.equal(fixture.location.href, undefined);
  }
});
