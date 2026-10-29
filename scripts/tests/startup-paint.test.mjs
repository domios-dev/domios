import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../../src-tauri/src/startup/frontend_shell/initial_document.js', import.meta.url), 'utf8');

function documentFixture({ readyState = 'loading', markWidth = 76, markHeight = 76, statusWidth = 100, statusHeight = 24, status = 'Opaque status', role = 'status', hasMark = true, framesThrow = true } = {}) {
  const events = new Map(), styles = [], navigations = [];
  const box = (width, height) => {
    assert.equal(styles.length, 1);
    return { width, height };
  };
  const main = {
    querySelector(selector) {
      if (selector === 'svg') return hasMark ? { getBoundingClientRect: () => box(markWidth, markHeight) } : null;
      if (selector === 'h1') return { textContent: status, getBoundingClientRect: () => box(statusWidth, statusHeight) };
      return null;
    },
  };
  const window = {};
  runInNewContext(source, {
    window,
    location: { set href(value) { navigations.push(value); } },
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
    requestAnimationFrame: () => { if (framesThrow) throw Error('Hidden view cannot deliver frames'); },
  });
  return { window, navigations, styles, events };
}

for (const role of ['status', 'alert']) {
  for (const framesThrow of [false, true]) {
    for (const readyState of ['loading', 'complete']) {
      test(`Starting ${role} acknowledges layout with frames unavailable: throws=${framesThrow}, document=${readyState}`, () => {
        const fixture = documentFixture({ role, framesThrow, readyState });
        fixture.window.__DOMIOS_STARTING_READY__(7);
        if (readyState === 'loading') {
          assert.deepEqual(fixture.navigations, []);
          assert.equal(fixture.styles.length, 0);
          fixture.events.get('DOMContentLoaded')();
        }
        assert.equal(fixture.styles.length, 1);
        assert.deepEqual(fixture.navigations, ['domios-starting-ready://localhost/7']);
        fixture.window.__DOMIOS_STARTING_READY__(7);
        assert.equal(fixture.navigations.length, 1);
      });
    }
  }
}

test('A replaced pending generation cannot acknowledge the old layout request', () => {
  const fixture = documentFixture();
  fixture.window.__DOMIOS_STARTING_READY__(1);
  fixture.window.__DOMIOS_STARTING_READY__(2);
  fixture.events.get('DOMContentLoaded')();
  assert.deepEqual(fixture.navigations, ['domios-starting-ready://localhost/2']);
});

test('Missing content and zero layout refuse document readiness and request native recovery', () => {
  for (const options of [{ status: '' }, { status: '  ' }, { hasMark: false }, { role: null }, { markWidth: 0 }, { markHeight: 0 }, { statusWidth: 0 }, { statusHeight: 0 }]) {
    const fixture = documentFixture({ ...options, readyState: 'complete' });
    fixture.window.__DOMIOS_STARTING_READY__(3);
    assert.deepEqual(fixture.navigations, ['domios-starting-unavailable://localhost/3']);
    fixture.window.__DOMIOS_STARTING_READY__(3);
    assert.equal(fixture.navigations.length, 1);
  }
});
