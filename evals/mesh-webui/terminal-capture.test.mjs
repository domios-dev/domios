import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { captureTerminal, readTerminalState } from './terminal-capture.mjs';

function fixture({ visible = true, count = 1, bufferType = 'normal' } = {}) {
  const cell = {
    getChars: () => '界', getWidth: () => 2, getFgColor: () => 0x123456,
    getBgColor: () => 0, getFgColorMode: () => 0x3000000, getBgColorMode: () => 0,
    isBold: () => 1, isItalic: () => 0, isUnderline: () => 0,
    isInverse: () => 0, isInvisible: () => 0,
  };
  const term = { cols: 1, rows: 1, options: { cursorStyle: 'bar', cursorBlink: false },
    _core: { coreService: { isCursorHidden: true } },
    buffer: { active: { type: bufferType, length: 1, baseY: 0, viewportY: 0,
      cursorX: 0, cursorY: 0,
      getLine: () => ({ length: 1, isWrapped: false, getCell: () => cell, translateToString: () => '界' }) } } };
  const host = { __codetermTerminal: term, getBoundingClientRect: () => ({ x: 2, y: 3, width: 80, height: 24 }) };
  const root = { dataset: { tabId: 'mesh:fixture/tab' }, getClientRects: () => [1],
    querySelector: selector => selector.includes('terminal-frame')
      ? { dataset: { terminalVisible: String(visible) } } : host };
  return { querySelectorAll: () => Array(count).fill(root) };
}

test('actual buffer Unicode, colors and alternate state survive serialization', () => {
  const original = globalThis.document;
  try {
    globalThis.document = fixture({ bufferType: 'alternate' });
    const state = readTerminalState({ tabId: 'mesh:fixture/tab' });
    assert.equal(state.buffer, 'alternate');
    assert.equal(state.lines[0].cells[0].chars, '界');
    assert.equal(state.lines[0].cells[0].fg, 0x123456);
    assert.equal(state.cursor.hidden, true);
  } finally { globalThis.document = original; }
});

test('hidden chat terminal and duplicate tab roots cannot supply renderer proof', () => {
  const original = globalThis.document;
  try {
    globalThis.document = fixture({ visible: false });
    assert.throws(() => readTerminalState({ tabId: 'mesh:fixture/tab' }), /hidden/);
    globalThis.document = fixture({ count: 2 });
    assert.throws(() => readTerminalState({ tabId: 'mesh:fixture/tab' }), /found 2/);
  } finally { globalThis.document = original; }
});

test('changing screenshot capture retains before and after state as failure evidence', async () => {
  const temp = await mkdtemp(join(tmpdir(), 'b3-capture-'));
  try {
    let call = 0;
    const states = [
      { truncated: false, viewportY: 0, rows: 1, lines: [{ y: 0, text: 'before' }] },
      { truncated: false, viewportY: 0, rows: 1, lines: [{ y: 0, text: 'after' }] },
    ];
    const page = {
      evaluate: async fn => fn === readTerminalState ? states[call++ ? 1 : 0] : (call === 1 ? 1 : [{ type: 'render', revision: 2 }]),
      screenshot: async () => {},
    };
    const directory = join(temp, 'capture');
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: { fixture: true } }), /changing capture/);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.unchanged, false);
    assert.equal(saved.before.lines[0].text, 'before');
    assert.equal(saved.after.lines[0].text, 'after');
  } finally { await rm(temp, { recursive: true, force: true }); }
});
