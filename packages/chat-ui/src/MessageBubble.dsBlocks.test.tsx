import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { act } from "react";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import { MessageBubble, MessagePartsContext, MessagePartView, RichMarkdown, ChatBlockHostProvider, LocalArtifactScope, configureHtmlFileLoader, binding, mount, html } from "./dsBlocks.testdata";

test("MessageBubble dsBlocks central causal fixture: own HTML close gates creation and twenty appends preserve exact frames and fullscreen", async () => {
  const view = await mount();
  const render = (content: string, streaming = true, fontScale = 1, id = "live") => view.render(<MessagePartsContext.Provider value={{ id, source: content }}><MessageBubble message={{ id, type: "assistant", content }} streaming={streaming} fontScale={fontScale} /></MessagePartsContext.Provider>);
  try {
    const closeAt = html.lastIndexOf('</ds-html>');
    for (const prefix of ['<', '<ds-', html.slice(0, closeAt), html.slice(0, -1)]) {
      await render(prefix);
      assert.equal(view.host.querySelectorAll("iframe").length, 0);
      assert.ok(view.host.querySelector('[data-ds-state="pending"]'));
      assert.equal(view.host.querySelectorAll("input,script,svg,button[onclick]").length, 0);
    }
    await render(html);
    const frame = view.host.querySelector<HTMLIFrameElement>('iframe');
    assert.ok(frame);
    const srcDoc = frame.getAttribute("srcdoc");
    assert.ok(srcDoc?.includes('<input/>') && srcDoc.includes('const outer="</ds-html>"'));
    const open = view.host.querySelector<HTMLButtonElement>('button[title="Open in fullscreen"]');
    assert.ok(open);
    await act(async () => open.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]');
    assert.ok(dialog);
    const fullscreenFrame = dialog.querySelector('iframe');
    assert.ok(fullscreenFrame);
    assert.equal(view.host.querySelector('iframe') === frame, true, 'fullscreen opened: inline frame identity');
    assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, 'fullscreen opened: inline srcDoc');
    let source = html;
    for (let index = 0; index < 20; index++) {
      source += `\n\nfollowing ${index}`;
      await render(source, true, index % 2 ? 1 : 2);
      assert.equal(view.host.querySelector('iframe') === frame, true, `append ${index}: inline frame identity`);
      assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, `append ${index}: inline srcDoc`);
      assert.equal(document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]') === dialog, true, `append ${index}: fullscreen dialog identity`);
      assert.equal(dialog.querySelector('iframe') === fullscreenFrame, true, `append ${index}: fullscreen frame identity`);
    }
    await render(source, false);
    assert.equal(view.host.querySelector('iframe') === frame, true, 'ended: inline frame identity');
    assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, 'ended: inline srcDoc');
    assert.equal(document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]') === dialog, true, 'ended: fullscreen dialog identity');
    assert.equal(dialog.querySelector('iframe') === fullscreenFrame, true, 'ended: fullscreen frame identity');
  } finally { await view.close(); }
});


test("dsBlocks message-part context association and selection gutters preserve physical frames and subscriptions", async () => {
  const view = await mount();
  const plugins = binding({ kind: "loading" } as const);
  const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
  const source = html + '\n<ds-plugin id="git" />';
  const render = (associated: boolean, selecting: boolean, present = true) => view.render(
    <ChatBlockHostProvider value={value}><MessagePartsContext.Provider value={present ? { id: "parts", source: associated ? source : source + '\nnext token', selecting } : null}>
      <MessagePartView data-message-part-id={associated && present ? "parts:0" : undefined}>
        <RichMarkdown text={source} sourceIdentity="part-association" streaming />
      </MessagePartView>
    </MessagePartsContext.Provider></ChatBlockHostProvider>,
  );
  try {
    await render(true, false);
    const frame = view.host.querySelector('iframe'); assert.ok(frame);
    const srcDoc = frame.getAttribute('srcdoc');
    const open = view.host.querySelector<HTMLButtonElement>('button[title="Open in fullscreen"]'); assert.ok(open);
    await act(async () => open.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]'); assert.ok(dialog);
    const fullscreenFrame = dialog.querySelector('iframe'); assert.ok(fullscreenFrame);
    assert.deepEqual(plugins.counts, { starts: 1, live: 1, refresh: 1 }, 'before context changes: binding lifetime');
    for (let index = 0; index < 20; index++) {
      await render(index % 2 === 0, index % 3 === 0, index % 4 !== 0);
      assert.equal(view.host.querySelector('iframe') === frame, true, `context ${index}: physical frame`);
      assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, `context ${index}: srcDoc`);
      assert.equal(document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]') === dialog, true, `context ${index}: fullscreen dialog`);
      assert.equal(dialog.querySelector('iframe') === fullscreenFrame, true, `context ${index}: fullscreen frame`);
      assert.deepEqual(plugins.counts, { starts: 1, live: 1, refresh: 1 }, `context ${index}: binding lifetime`);
    }
  } finally { await view.close(); }
  assert.equal(plugins.counts.live, 0);
});


test("MessageBubble dsBlocks source and tab authority replacements reset frames", async () => {
  const view = await mount();
  const render = (text: string, tab = "one") => view.render(<LocalArtifactScope tabId={tab}><RichMarkdown text={text} sourceIdentity="same" streaming /></LocalArtifactScope>);
  try {
    await render(html); const frame = view.host.querySelector('iframe'); assert.ok(frame);
    await render('<ds-html><b>replacement</b></ds-html>'); const replaced = view.host.querySelector('iframe'); assert.ok(replaced); assert.equal(replaced === frame, false, 'replacement: frame identity');
    await render('<ds-html><b>replacement</b></ds-html>', "two"); assert.equal(view.host.querySelector('iframe') === replaced, false, 'authority replacement: frame identity');
  } finally { await view.close(); }
});


test("dsBlocks direct abandoned suffix to different actual suffix preserves committed effects and fullscreen", async () => {
  const view = await mount();
  const pending = new Promise<void>(() => {});
  let suspended = 0; let reads = 0;
  const plugins = binding({ kind: "loading" } as const);
  const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
  configureHtmlFileLoader(async () => { reads++; return '<b>Committed local page</b>'; });
  function Wait(): React.ReactNode { suspended++; throw pending; }
  const committed = html + '\n<ds-plugin id="git" />\n<ds-page src="/committed.html" />';
  const render = (text: string, suspend = false) => <ChatBlockHostProvider value={value}><React.Suspense fallback={<span>Deferred</span>}><RichMarkdown text={text} sourceIdentity="concurrent" streaming />{suspend && <Wait />}</React.Suspense></ChatBlockHostProvider>;
  try {
    await view.render(render(committed));
    const frames = [...view.host.querySelectorAll('iframe')]; assert.equal(frames.length, 2);
    const documents = frames.map((frame) => frame.getAttribute('srcdoc'));
    assert.equal(reads, 1); assert.equal(plugins.counts.starts, 1); assert.equal(plugins.counts.live, 1);
    const open = view.host.querySelector<HTMLButtonElement>('button[title="Open in fullscreen"]'); assert.ok(open); await act(async () => open.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]'); assert.ok(dialog);
    await view.transition(render(committed + '\n\nabandoned suffix', true));
    assert.ok(suspended > 0);
    let actual = committed + '\n\ndifferent actual suffix';
    for (let after = 0; after < 20; after++) {
      await view.render(render(actual));
      const actualFrames = [...view.host.querySelectorAll('iframe')];
      assert.equal(actualFrames.length, frames.length, `abandoned suffix append ${after}: frame count`);
      for (const [frameIndex, frame] of frames.entries()) assert.equal(actualFrames[frameIndex] === frame, true, `abandoned suffix append ${after}: frame ${frameIndex} identity`);
      for (const [frameIndex, frame] of frames.entries()) assert.equal(frame.getAttribute('srcdoc') === documents[frameIndex], true, `abandoned suffix append ${after}: frame ${frameIndex} srcDoc`);
      assert.equal(document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]') === dialog, true, `abandoned suffix append ${after}: fullscreen dialog identity`);
      assert.equal(reads, 1); assert.equal(plugins.counts.starts, 1); assert.equal(plugins.counts.live, 1);
      actual += `\n\ncontinuation ${after}`;
    }
  } finally { await view.close(); configureHtmlFileLoader(null); }
});


test("dsBlocks HTML text states cannot expose a nested file or frame before the true data-state close", async () => {
  const view = await mount();
  let reads = 0;
  configureHtmlFileLoader(async () => { reads++; return '<b>Forbidden nested file</b>'; });
  try {
    for (const kind of ['xmp', 'iframe', 'noembed', 'noframes', 'noscript']) {
      const prefix = `<ds-html><${kind}>\n</ds-html>\n<ds-page src="/private-example.html" />\n</${kind}>\n`;
      const closer = '</ds-html>';
      for (let end = 0; end < closer.length; end++) {
        await view.render(<RichMarkdown key={kind} text={prefix + closer.slice(0, end)} sourceIdentity={kind} streaming />);
        assert.equal(reads, 0); assert.equal(view.host.querySelectorAll('iframe').length, 0);
      }
      await view.render(<RichMarkdown key={kind} text={prefix + '</ds-html>'} sourceIdentity={kind} streaming />);
      assert.equal(reads, 0); assert.equal(view.host.querySelectorAll('iframe').length, 1);
      assert.ok(view.host.querySelector('iframe')?.getAttribute('srcdoc')?.includes('/private-example.html'));
    }
    const plaintext = '<ds-html><plaintext>\n</ds-html>\n<ds-page src="/private-example.html" />\n</plaintext>\n</ds-html>';
    await view.render(<RichMarkdown key="plaintext" text={plaintext} streaming />);
    await view.render(<RichMarkdown key="plaintext" text={plaintext} streaming={false} />);
    assert.equal(reads, 0); assert.equal(view.host.querySelectorAll('iframe').length, 0);
    assert.ok(view.host.querySelector('[data-ds-state="malformed"]'));
  } finally { await view.close(); configureHtmlFileLoader(null); }
});


test("dsBlocks abandoned finish or cancellation cannot poison a committed pending candidate", async () => {
  const view = await mount();
  const pending = new Promise<void>(() => {});
  let suspended = 0;
  function Wait(): React.ReactNode { suspended++; throw pending; }
  const prefix = '<ds-html><input/>';
  const render = (id: string, text: string, streaming: boolean, cancelled: boolean, suspend = false) => <React.Suspense fallback={<span>Deferred</span>}><RichMarkdown key={id} text={text} sourceIdentity={id} streaming={streaming} cancelled={cancelled} />{suspend && <Wait />}</React.Suspense>;
  try {
    for (const lifecycle of ['ended', 'cancelled'] as const) {
      await view.render(render(lifecycle, prefix, true, false));
      const count = suspended;
      await view.transition(render(lifecycle, prefix, lifecycle !== 'ended', lifecycle === 'cancelled', true));
      assert.ok(suspended > count);
      await view.render(render(lifecycle, prefix + '</ds-html>', true, false));
      assert.equal(view.host.querySelectorAll('iframe').length, 1);
      assert.ok(view.host.querySelector('iframe')?.getAttribute('srcdoc')?.includes('<input/>'));
    }
  } finally { await view.close(); }
});

