import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import { mermaidCalls, MessageBubble, RichMarkdown, ChatBlockHostProvider, configureHtmlFileLoader, binding, mount, html } from "./dsBlocks.testdata";

test("dsBlocks retires executable HTML fences and gates Mermaid on the complete fence", async () => {
  const view = await mount();
  let reads = 0; configureHtmlFileLoader(async () => { reads++; return '<b>Unexpected</b>'; });
  try {
    for (const text of ['```html\n<b>Code only</b>', '```html\n@file /a.html', '```html\n<b>Code only</b>\n```', '```html\n@file /a.html\n```']) {
      await view.render(<RichMarkdown text={text} />);
      assert.equal(view.host.querySelectorAll('iframe').length, 0); assert.ok(view.host.querySelector('[data-testid="chat-code-block"]')); assert.equal(reads, 0);
    }
    const count = mermaidCalls.length;
    const source = '```mermaid\ngraph TD; causalA-->causalB\n```';
    await view.render(<RichMarkdown key="diagram" text={source.slice(0, -1)} streaming />);
    assert.equal(mermaidCalls.length, count);
    await view.render(<RichMarkdown key="diagram" text={source} streaming />);
    assert.equal(mermaidCalls.length, count);
    await view.render(<RichMarkdown key="diagram" text={source} streaming={false} />);
    assert.equal(mermaidCalls.length, count + 1);
  } finally { await view.close(); configureHtmlFileLoader(null); }
});


test("dsBlocks tool and paste excerpts never create frames or agent subscriptions and release outside blocks", async () => {
  for (const wrapper of ["tool_result", "tool_output", "transcript", "pasted_content"]) {
    const view = await mount();
    const agents = binding({ kind: "loading" } as const);
    const value: ChatBlockHost = { agents: { binding: agents.source, open() {}, stop: async () => {} } };
    const excerpt = `<${wrapper}>\n${html}\n<ds-agent tab="deadbeef" />\n</${wrapper}>`;
    const render = (content: string, streaming = true) => view.render(<ChatBlockHostProvider value={value}><MessageBubble message={{ id: wrapper, type: "assistant", content }} streaming={streaming} /></ChatBlockHostProvider>);
    try {
      const boundaries = [1, wrapper.length + 2, excerpt.indexOf('</ds-html>') + '</ds-html>'.length, excerpt.indexOf('<ds-agent') + '<ds-agent tab="deadbeef" />'.length, excerpt.length - 1, excerpt.length];
      for (const end of boundaries) {
        await render(excerpt.slice(0, end));
        assert.equal(view.host.querySelectorAll("iframe").length, 0, `${wrapper} prefix ${end}: frames`);
        assert.deepEqual(agents.counts, { starts: 0, live: 0, refresh: 0 }, `${wrapper} prefix ${end}: agent effects`);
      }
      const source = `${excerpt}\n${html}\n<ds-agent tab="outside" />`;
      await render(source);
      const frame = view.host.querySelector("iframe");
      assert.ok(frame);
      assert.equal(view.host.querySelectorAll("iframe").length, 1);
      assert.deepEqual(agents.counts, { starts: 1, live: 1, refresh: 1 });
      await render(source, false);
      assert.equal(view.host.querySelector("iframe"), frame);
      assert.deepEqual(agents.counts, { starts: 1, live: 1, refresh: 1 });
    } finally { await view.close(); }
    assert.equal(agents.counts.live, 0);
  }
});


test("MessageBubble dsBlocks pending body cannot create plan links, mention subscriptions or navigation actions", async () => {
  const view = await mount();
  let resolves = 0; let navigations = 0;
  const mentions = binding(null);
  const value: ChatBlockHost = { mentionTabId: "owner", mentions: { binding: mentions.source, open() { navigations++; } } };
  try {
    for (const content of ['<ds-html><a href="https://example.test">pl_alpha</a> task_alpha', '<ds-html><a href="https://example.test">pl_alpha</a> task_alpha</ds-html>']) {
      await view.render(<ChatBlockHostProvider value={value}><MessageBubble message={{ id: "references", type: "assistant", content }} streaming onOpenPlan={() => { navigations++; }} resolvePlanRef={() => { resolves++; return null; }} /></ChatBlockHostProvider>);
      assert.equal(view.host.querySelectorAll('[data-testid="chat-plan-ref-cards"],a').length, 0);
      assert.equal(resolves, 0); assert.equal(mentions.counts.starts, 0); assert.equal(navigations, 0);
    }
  } finally { await view.close(); }
});


test("dsBlocks scanner fence spans gate existing mmd and inferred Mermaid without a renderer closing parser", async () => {
  const view = await mount();
  try {
    for (const info of ['mmd', '']) {
      const before = mermaidCalls.length;
      const name = info ? 'aliasDiagram' : 'inferredDiagram';
      const source = '```' + info + '\ngraph TD\n  ' + name + '-->complete\n```';
      await view.render(<RichMarkdown key={name} text={source.slice(0, -1)} streaming />);
      assert.equal(mermaidCalls.length, before);
      await view.render(<RichMarkdown key={name} text={source} streaming />);
      assert.equal(mermaidCalls.length, before);
      await view.render(<RichMarkdown key={name} text={source} streaming={false} />);
      assert.equal(mermaidCalls.length, before + 1);
    }
  } finally { await view.close(); }
});


test("dsBlocks LinkedText and user boundaries hide pending/malformed bodies while outside mentions keep their actions", async () => {
  const view = await mount();
  const { LinkedText } = await import("./codeterm/LinkedText");
  const { DsRenderContext } = await import("./blocks/DsRenderContext");
  const reads = binding({ title: "Outside task", kind: "task", id: "task_outside" } as const);
  const opened: string[] = [];
  const value: ChatBlockHost = { mentionTabId: "owner", mentions: { binding: reads.source, open: (key) => opened.push(key) } };
  const pending = '<ds-html><b>task_hidden</b>';
  const complete = pending + '</ds-html>';
  const malformed = '<ds-html bad="attribute"><b>task_hidden</b></ds-html>';
  try {
    for (const surface of ['direct', 'user', 'attachment'] as const) {
      let sequence = 0;
      const render = (source: string, trusted = true, streaming = true, cancelled = false) => {
        const id = `${surface}-${sequence}`;
        const text = surface === 'direct' ? <LinkedText text={source} /> : <MessageBubble message={{ id, type: "user", content: source, ...(surface === 'attachment' ? { segments: [{ type: "attachment" as const, kind: "file", path: "/genuine.txt", label: "Genuine", body: null, status: null }, { type: "text" as const, text: '\n' + source }] } : {}) }} streaming={streaming} origin={trusted ? undefined : { kind: "neighbor", friendshipId: "friend" }} />;
        return view.render(<ChatBlockHostProvider value={value}><DsRenderContext.Provider value={{ authority: id, trusted, streaming, cancelled }}>{text}</DsRenderContext.Provider></ChatBlockHostProvider>);
      };
      for (const source of [pending, malformed]) {
        sequence++; const before = reads.counts.starts;
        await render(source); assert.equal(reads.counts.starts, before);
        await render(source, true, false); assert.equal(reads.counts.starts, before);
      }
      sequence++; const before = reads.counts.starts;
      await render(pending); await render(pending, true, true, true); await render(complete, true, true, true);
      assert.equal(reads.counts.starts, before); assert.equal(view.host.querySelectorAll("iframe").length, 0);
      sequence++; await render(complete, false); assert.equal(reads.counts.starts, before);
      sequence++; await render('<domios type="task" id="task_outside" />\n\n' + pending);
      assert.equal(reads.counts.starts, before + 1);
      const chip = view.host.querySelector<HTMLButtonElement>('[data-testid="chat-task-link"]'); assert.ok(chip && !chip.disabled);
      const opens = opened.length; await act(async () => chip.click()); assert.equal(opened.length, opens + 1); assert.ok(opened[opened.length - 1].includes('task_outside'));
      if (surface === 'attachment') assert.ok(view.host.querySelector('[data-testid="chat-inline-attachment"]'));
    }
  } finally { await view.close(); }
});

