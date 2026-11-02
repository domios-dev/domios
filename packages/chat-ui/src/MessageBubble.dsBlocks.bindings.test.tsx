import assert from "node:assert/strict";
import test from "node:test";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import { MessageBubble, RichMarkdown, ChatBlockHostProvider, configureHtmlFileLoader, binding, mount, html } from "./dsBlocks.testdata";

test("MessageBubble dsBlocks live cards subscribe only after own terminator, once across later prose and stacked cards", async () => {
  const view = await mount();
  const plugins = binding({ kind: "loading" } as const);
  const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
  const render = (source: string, id = "live", trusted = true) => view.render(<ChatBlockHostProvider value={value}><RichMarkdown text={source} sourceIdentity={id} interactivePreviews={trusted} streaming /></ChatBlockHostProvider>);
  const tag = '<ds-plugin id="git" />';
  try {
    for (let at = 1; at < tag.length; at++) { await render(tag.slice(0, at)); assert.equal(plugins.counts.starts, 0); }
    await render(tag); assert.deepEqual(plugins.counts, { starts: 1, live: 1, refresh: 1 });
    let source = tag;
    for (let index = 0; index < 20; index++) { source += `\n\nmore ${index}`; await render(source); assert.equal(plugins.counts.starts, 1); assert.equal(plugins.counts.live, 1); }
    await render(source + '\n<ds-plugin id="second" />'); assert.equal(plugins.counts.starts, 2); assert.equal(plugins.counts.live, 2);
    await render(tag, "authority-two"); assert.equal(plugins.counts.live, 1); assert.equal(plugins.counts.starts, 3);
    await render(tag, "authority-two", false); assert.equal(plugins.counts.live, 0); assert.equal(plugins.counts.starts, 3);
    await render('<ds-plugin id="git" id="other" />', "malformed"); assert.equal(plugins.counts.live, 0); assert.equal(plugins.counts.starts, 3);
  } finally { await view.close(); }
  assert.equal(plugins.counts.live, 0);
});


test("dsBlocks all nine live-card kinds have no subscription before their own terminator", async () => {
  const view = await mount();
  const events: string[] = [];
  let stage = "initial";
  const record = (event: string) => { events.push(`${stage}:${event}`); if (events.length > 12) events.shift(); };
  const loading = binding({ kind: "loading" } as const, record);
  const checklist = binding({ kind: "missing" } as const, record);
  const choice = binding({ reply: null }, record);
  const value: ChatBlockHost = {
    plugins: { binding: loading.source, run: async () => {}, openView() {}, openInPlugins() {} },
    connect: { binding: loading.source, connect: async () => {}, disconnect: async () => {}, openSettings() {} },
    settings: { binding: loading.source, write: async () => {} },
    agents: { binding: loading.source, open() {}, stop: async () => {} },
    jobs: { binding: loading.source, open() {}, runNow: async () => {}, setPaused: async () => {}, stop: async () => {} },
    checklists: { binding: checklist.source, open() {} },
    tasks: { binding: loading.source, openPlan() {}, openTask() {} },
    choice: { binding: choice.source, choose: async () => true },
  };
  const tags = ['<ds-guide section="phone" />', '<ds-plugin id="git" />', '<ds-connect bridge="telegram" />', '<ds-setting key="plugins.git.enabled" />', '<ds-agent tab="e3ec9ee9" />', '<ds-job ref="nightly" />', '<ds-checklist id="getting-started" />', '<ds-choice options=\'["Yes","No"]\' />', '<ds-tasks plan="pl_alpha" />'];
  const render = (text: string, id: string) => view.render(<ChatBlockHostProvider value={value}><MessageBubble message={{ id, type: "assistant", content: text }} streaming /></ChatBlockHostProvider>);
  const starts = () => loading.counts.starts + checklist.counts.starts + choice.counts.starts;
  const live = () => loading.counts.live + checklist.counts.live + choice.counts.live;
  const refreshes = () => loading.counts.refresh + checklist.counts.refresh + choice.counts.refresh;
  try {
    for (const [index, tag] of tags.entries()) {
      const initial = starts();
      for (const at of [1, 4, tag.length - 2, tag.length - 1]) {
        stage = `kind ${index} ${tag} prefix ${at}`; await render(tag.slice(0, at), `kind-${index}`);
        assert.equal(starts(), initial, `${stage}: ${events.join(" | ")}`);
        assert.equal(live(), 0); assert.equal(refreshes(), initial);
        assert.equal(view.host.querySelectorAll('[data-chat-block]').length, 0);
      }
      stage = `kind ${index} ${tag} complete`;
      await render(tag, `kind-${index}`);
      const active = index === 0 ? 0 : 1;
      const card = view.host.querySelector(`[data-chat-block="${tag.slice(4, tag.indexOf(' '))}"]`); assert.ok(card);
      assert.equal(starts(), initial + active, `${stage}: ${events.join(" | ")}`);
      assert.equal(live(), active); assert.equal(refreshes(), initial + active);
      for (let after = 0; after < 2; after++) {
        stage = `kind ${index} ${tag} append ${after}`; await render(tag + '\n\n' + 'more '.repeat(after + 1), `kind-${index}`);
        assert.equal(starts(), initial + active, `${stage}: ${events.join(" | ")}`);
        assert.equal(live(), active); assert.equal(refreshes(), initial + active);
        assert.equal(view.host.querySelector('[data-chat-block]'), card);
      }
    }
  } finally { await view.close(); }
  assert.equal(live(), 0);
});


test("dsBlocks MessageBubble never rebinds preceding deferred content to a new message or replacement prefix", async () => {
  const view = await mount();
  try {
    for (const replacement of ['new-message', 'same-message'] as const) {
      const events: string[] = [];
      const plugins = binding({ kind: "loading" } as const, (event) => events.push(event));
      const connect = binding({ kind: "loading" } as const);
      const value: ChatBlockHost = {
        plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} },
        connect: { binding: connect.source, connect: async () => {}, disconnect: async () => {}, openSettings() {} },
      };
      const render = (content: string, id: string) => view.render(<ChatBlockHostProvider value={value}><MessageBubble message={{ id, type: "assistant", content }} streaming /></ChatBlockHostProvider>);
      await render('<ds-plugin id="git" />', 'preceding');
      assert.deepEqual(plugins.counts, { starts: 1, live: 1, refresh: 1 }, `${replacement}: preceding plugin`);
      const tag = '<ds-connect bridge="telegram" />';
      const id = replacement === 'new-message' ? 'candidate' : 'preceding';
      for (let at = 1; at < tag.length; at++) {
        await render(tag.slice(0, at), id);
        assert.deepEqual(plugins.counts, { starts: 1, live: 0, refresh: 1 }, `${replacement} prefix ${at}: preceding lifetime`);
        assert.deepEqual(events, ['subscribe', 'refresh:git', 'unsubscribe'], `${replacement} prefix ${at}: preceding events`);
        assert.deepEqual(connect.counts, { starts: 0, live: 0, refresh: 0 }, `${replacement} prefix ${at}: candidate lifetime`);
        assert.equal(view.host.querySelectorAll('[data-chat-block="plugin"],[data-chat-block="connect"]').length, 0);
      }
      await render(tag, id);
      assert.deepEqual(plugins.counts, { starts: 1, live: 0, refresh: 1 }, `${replacement}: prior plugin stays released`);
      assert.deepEqual(connect.counts, { starts: 1, live: 1, refresh: 1 }, `${replacement}: own terminator`);
      for (let after = 0; after < 20; after++) {
        await render(tag + '\n\n' + 'more '.repeat(after + 1), id);
        assert.deepEqual(connect.counts, { starts: 1, live: 1, refresh: 1 }, `${replacement} append ${after}: candidate lifetime`);
      }
      await view.render(null);
      assert.equal(connect.counts.live, 0);
    }
  } finally { await view.close(); }
});


test("dsBlocks user task snapshots obey explicit trust and committed stream lifecycle without a parallel extractor", async () => {
  const view = await mount();
  const { DsRenderContext } = await import("./blocks/DsRenderContext");
  let reads = 0; let pageReads = 0;
  const tasks = binding({ kind: "loading" } as const);
  const read = tasks.source.read;
  const value: ChatBlockHost = { tasks: { binding: { ...tasks.source, read(key) { reads++; return read(key); } }, openPlan() {}, openTask() {} } };
  configureHtmlFileLoader(async () => { pageReads++; return '<b>Existing page</b>'; });
  const prefix = '<ds-tasks plan="pl_x">{"title":"X","items":[]}';
  const source = prefix + '</ds-tasks>';
  const render = (text: string, id: string, trusted = true, streaming = true, cancelled = false) => view.render(<ChatBlockHostProvider value={value}><DsRenderContext.Provider value={{ authority: id, trusted, streaming, cancelled }}><MessageBubble message={{ id, type: "user", content: text }} streaming={streaming} /></DsRenderContext.Provider></ChatBlockHostProvider>);
  try {
    await render(source, 'untrusted-snapshot', false);
    assert.equal(tasks.counts.starts, 0); assert.equal(reads, 0); assert.equal(pageReads, 0);
    assert.equal(view.host.querySelectorAll('[data-chat-block="tasks"],iframe').length, 0);
    for (const lifecycle of ['ended', 'cancelled'] as const) {
      await render(prefix, lifecycle);
      await render(prefix, lifecycle, true, lifecycle !== 'ended', lifecycle === 'cancelled');
      await render(source, lifecycle, true, true, lifecycle === 'cancelled');
      assert.equal(tasks.counts.starts, 0); assert.equal(reads, 0); assert.equal(pageReads, 0);
      assert.equal(view.host.querySelectorAll('[data-chat-block="tasks"],iframe').length, 0);
    }
    const page = '<ds-page src="/existing-page.html" />\n';
    await render(page + prefix, 'trusted-snapshot');
    const frame = view.host.querySelector('iframe'); assert.ok(frame); assert.equal(pageReads, 1);
    assert.equal(tasks.counts.starts, 0); assert.equal(reads, 0);
    await render(page + source, 'trusted-snapshot');
    assert.equal(view.host.querySelector('iframe') === frame, true, 'task completion: frame identity'); assert.equal(pageReads, 1);
    assert.equal(view.host.querySelectorAll('[data-chat-block="tasks"]').length, 1);
    assert.equal(tasks.counts.starts, 1); assert.equal(tasks.counts.live, 1); assert.ok(reads > 0);
    for (let after = 0; after < 20; after++) {
      await render(page + source + '\n\n' + 'later '.repeat(after + 1), 'trusted-snapshot');
      assert.equal(view.host.querySelector('iframe') === frame, true, `task append ${after}: frame identity`); assert.equal(pageReads, 1); assert.equal(tasks.counts.starts, 1); assert.equal(tasks.counts.live, 1);
    }
  } finally { await view.close(); configureHtmlFileLoader(null); }
});

