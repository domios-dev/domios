import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import type { LiveBindingSource } from "./blocks/liveBinding";

const browser = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLIFrameElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? browser : browser[key], configurable: true });
}
browser.matchMedia = (() => ({ matches: false, media: "", onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => false })) as typeof browser.matchMedia;
const mermaidCalls: string[] = [];
Object.assign(browser, { mermaid: { initialize() {}, async render(_id: string, source: string) { mermaidCalls.push(source); return { svg: '<svg xmlns="http://www.w3.org/2000/svg"><text>Diagram</text></svg>' }; } } });
const { MessageBubble } = await import("./MessageBubble");
const { MessagePartsContext, MessagePartView } = await import("./MessagePartsContext");
const { RichMarkdown, renderUserContent } = await import("./MarkdownBody");
const { ChatBlockHostProvider } = await import("./blocks/ChatBlockHost");
const { LocalArtifactScope } = await import("./LocalArtifactScope");
const { configureHtmlFileLoader } = await import("./htmlFileLoader");
const { configureLocalArtifactLoader } = await import("./localArtifactLoader");

function binding<T>(value: T, event?: (value: string) => void) {
  const counts = { starts: 0, live: 0, refresh: 0 };
  const source: LiveBindingSource<T> = { read: () => value, refresh: (key) => { counts.refresh++; event?.(`refresh:${key}`); }, subscribe: () => { counts.starts++; counts.live++; event?.("subscribe"); return () => { counts.live--; event?.("unsubscribe"); }; } };
  return { source, counts };
}
async function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  return { host, render: (ui: React.ReactNode) => act(async () => root.render(ui)), transition: (ui: React.ReactNode) => act(async () => React.startTransition(() => root.render(ui))), close: async () => { await act(async () => root.unmount()); host.remove(); } };
}
const html = '<ds-html><input/><svg><path/></svg><script>const outer="</ds-html>";</script><button>Local action</button></ds-html>';

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

test("MessageBubble dsBlocks file effects occur once after terminator and survive twenty later chunks", async () => {
  const view = await mount();
  const reads = { page: 0, image: 0, document: 0 };
  configureHtmlFileLoader(async () => { reads.page++; return '<main>Local</main><script>removeMe()</script>'; });
  configureLocalArtifactLoader(async (_path, kind) => {
    if (kind === "image") { reads.image++; return { data: "data:image/png;base64,AA==", media_type: "image/png" }; }
    reads.document++; return { data: "# Local document", media_type: "text/markdown" };
  });
  const render = (text: string) => view.render(<LocalArtifactScope tabId="owner"><MessageBubble message={{ id: "files", type: "assistant", content: text }} streaming /></LocalArtifactScope>);
  try {
    const tags = ['<ds-page src="C:\\work\\a b.html" />', '<ds-image sources=\'["/one.png","/two.png"]\' />', '<ds-document src="/notes.md" />'];
    let source = "";
    for (const tag of tags) {
      const before = { ...reads };
      for (let at = 1; at < tag.length; at++) {
        await render(source + tag.slice(0, at));
        assert.deepEqual(reads, before);
      }
      source += tag + "\n"; await render(source);
    }
    assert.deepEqual(reads, { page: 1, image: 2, document: 1 });
    const frame = view.host.querySelector('iframe'); const srcDoc = frame?.getAttribute('srcdoc');
    assert.ok(frame && srcDoc?.includes('Local') && !srcDoc.includes('removeMe()'));
    const images = [...view.host.querySelectorAll('img')];
    for (let index = 0; index < 20; index++) {
      source += `\nfollowing ${index}`; await render(source);
      assert.equal(view.host.querySelector('iframe') === frame, true, `file append ${index}: frame identity`);
      assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, `file append ${index}: srcDoc`);
      const actualImages = [...view.host.querySelectorAll('img')];
      assert.equal(actualImages.length, images.length, `file append ${index}: image count`);
      for (const [imageIndex, image] of images.entries()) assert.equal(actualImages[imageIndex] === image, true, `file append ${index}: image ${imageIndex} identity`);
      assert.deepEqual(reads, { page: 1, image: 2, document: 1 });
    }
  } finally { await view.close(); configureHtmlFileLoader(null); configureLocalArtifactLoader(null); }
});

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

test("MessageBubble dsBlocks malformed, cancelled and Neighbor content have zero file, frame, subscription and diagram effects", async () => {
  const view = await mount();
  let reads = 0;
  const plugins = binding({ kind: "loading" } as const);
  const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
  configureHtmlFileLoader(async () => { reads++; return '<main>Denied</main>'; });
  configureLocalArtifactLoader(async () => { reads++; return { data: "data:image/png;base64,AA==", media_type: "image/png" }; });
  const diagramCount = mermaidCalls.length;
  try {
    for (const source of ['<ds-html><input/>', '<ds-page src="/page.html" /', '<ds-image src="/a.png" src="/b.png" />', '<ds-plugin id="git" duplicate="x" />', '<ds-html bad="x">\n```mermaid\ngraph TD; A-->B\n```\n</ds-html>']) {
      await view.render(<ChatBlockHostProvider value={value}><RichMarkdown key={source} text={source} cancelled streaming /></ChatBlockHostProvider>);
      assert.equal(view.host.querySelectorAll('iframe,img').length, 0); assert.equal(reads, 0); assert.equal(plugins.counts.starts, 0);
    }
    const complete = html + '\n<ds-page src="/page.html" />\n<ds-image src="/a.png" />\n<ds-plugin id="git" />\n```mermaid\ngraph TD; A-->B\n```';
    await view.render(<ChatBlockHostProvider value={value}><RichMarkdown text={complete} interactivePreviews={false} /></ChatBlockHostProvider>);
    assert.equal(view.host.querySelectorAll('iframe,img,script').length, 0); assert.equal(reads, 0); assert.equal(plugins.counts.starts, 0);
    assert.equal(mermaidCalls.length, diagramCount);
    const prefix = '<ds-html><input/>';
    await view.render(<RichMarkdown key="cancel" text={prefix} streaming />);
    await view.render(<RichMarkdown key="cancel" text={prefix} cancelled />);
    await view.render(<RichMarkdown key="cancel" text={prefix + '</ds-html>'} cancelled />);
    assert.equal(view.host.querySelectorAll('iframe').length, 0);
  } finally { await view.close(); configureHtmlFileLoader(null); configureLocalArtifactLoader(null); }
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
  try {
    for (const [index, tag] of tags.entries()) {
      const initial = starts();
      for (let at = 1; at < tag.length; at++) { stage = `kind ${index} ${tag} prefix ${at}`; await render(tag.slice(0, at), `kind-${index}`); assert.equal(starts(), initial, `${stage}: ${events.join(" | ")}`); }
      stage = `kind ${index} ${tag} complete`;
      await render(tag, `kind-${index}`);
      assert.equal(starts(), initial + (index === 0 ? 0 : 1), `${stage}: ${events.join(" | ")}`);
      for (let after = 0; after < 20; after++) { stage = `kind ${index} ${tag} append ${after}`; await render(tag + '\n\n' + 'more '.repeat(after + 1), `kind-${index}`); assert.equal(starts(), initial + (index === 0 ? 0 : 1), `${stage}: ${events.join(" | ")}`); }
    }
  } finally { await view.close(); }
});

test("dsBlocks beside genuine attachments use message trust and retain authority/opening-offset identity", async () => {
  const view = await mount();
  const { DsRenderContext } = await import("./blocks/DsRenderContext");
  const { InlineAttachmentText } = await import("./AttachmentCapsule");
  const { configureLocalArtifactOpener } = await import("./localArtifactLoader");
  const reads: string[] = []; const opens: string[] = [];
  configureHtmlFileLoader(async (path) => { reads.push(path); return '<button>Local interaction</button>'; });
  configureLocalArtifactLoader(async (path) => { reads.push(path); return { data: "Genuine file contents", media_type: "text/plain" }; });
  configureLocalArtifactOpener(async (path) => { opens.push(path); });
  const file = { kind: "file", path: "/genuine.txt", label: "Genuine" } as const;
  const tag = '<ds-page src="/directive.html" />';
  const render = (text: string, trusted = true, authority = "one") => view.render(<LocalArtifactScope tabId="owner"><DsRenderContext.Provider value={{ authority, trusted, streaming: true }}><InlineAttachmentText segments={[{ attachment: file }, { text: '\n' + text }]} /></DsRenderContext.Provider></LocalArtifactScope>);
  try {
    for (let at = 1; at < tag.length; at++) { await render(tag.slice(0, at)); assert.equal(reads.filter((path) => path === '/directive.html').length, 0); assert.equal(view.host.querySelectorAll('iframe').length, 0); }
    await render(tag);
    const frame = view.host.querySelector('iframe'); const srcDoc = frame?.getAttribute('srcdoc');
    const capsule = view.host.querySelector<HTMLButtonElement>('[data-testid="chat-inline-attachment"]');
    assert.ok(frame && capsule && !capsule.disabled);
    await act(async () => capsule.click()); assert.deepEqual(opens, ['/genuine.txt']);
    for (let after = 0; after < 20; after++) {
      await render(tag + '\n\n' + 'later '.repeat(after + 1));
      assert.equal(view.host.querySelector('iframe') === frame, true, `attachment append ${after}: frame identity`); assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, `attachment append ${after}: srcDoc`);
      assert.equal(view.host.querySelector('[data-testid="chat-inline-attachment"]') === capsule, true, `attachment append ${after}: capsule identity`);
      assert.equal(reads.filter((path) => path === '/directive.html').length, 1);
    }
    await render(tag, false);
    assert.equal(view.host.querySelectorAll('iframe').length, 0);
    assert.equal(reads.filter((path) => path === '/directive.html').length, 1);
    assert.equal(view.host.querySelector('[data-testid="chat-inline-attachment"]') === capsule, true, 'untrusted: capsule identity');
    await render(tag, true, 'two'); assert.equal(view.host.querySelector('iframe') === frame, false, 'attachment authority replacement: frame identity');
    assert.equal(reads.filter((path) => path === '/directive.html').length, 2);
    await view.render(<LocalArtifactScope tabId="owner"><MessageBubble message={{ id: "neighbor", type: "user", content: tag, segments: [{ type: "attachment", kind: "file", path: file.path, label: file.label, body: null, status: null }, { type: "text", text: '\n' + tag }] }} origin={{ kind: "neighbor", friendshipId: "friend" }} streaming /></LocalArtifactScope>);
    assert.equal(view.host.querySelectorAll('iframe').length, 0);
    assert.equal(reads.filter((path) => path === '/directive.html').length, 2);
    assert.ok(view.host.textContent?.includes('<ds-page'));
    assert.ok(view.host.querySelector('[data-testid="chat-inline-attachment"]'));
  } finally { await view.close(); configureHtmlFileLoader(null); configureLocalArtifactLoader(null); configureLocalArtifactOpener(null); }
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
      sequence++; await render('task_outside\n\n' + pending);
      assert.equal(reads.counts.starts, before + 1);
      const chip = view.host.querySelector<HTMLButtonElement>('[data-testid="chat-task-link"]'); assert.ok(chip && !chip.disabled);
      const opens = opened.length; await act(async () => chip.click()); assert.equal(opened.length, opens + 1); assert.ok(opened[opened.length - 1].includes('task_outside'));
      if (surface === 'attachment') assert.ok(view.host.querySelector('[data-testid="chat-inline-attachment"]'));
    }
  } finally { await view.close(); }
});

test("dsBlocks public whole-text adapters replace longer non-prefix snapshots under unchanged logical ids", async () => {
  const view = await mount();
  const { LinkedText } = await import("./codeterm/LinkedText");
  const { InlineAttachmentText } = await import("./AttachmentCapsule");
  const { DsRenderContext } = await import("./blocks/DsRenderContext");
  const oldSource = '<ds-page src="/old-snapshot.html" />\n<ds-plugin id="git" />';
  const newSource = 'Corrected content ' + 'larger replacement '.repeat(20) + '\n<ds-html><button>Replacement</button></ds-html>\n<ds-plugin id="second" />';
  try { for (const surface of ['rich', 'linked', 'assistant', 'user', 'attachment'] as const) {
    let pageReads = 0;
    const plugins = binding({ kind: "loading" } as const);
    const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
    configureHtmlFileLoader(async () => { pageReads++; return '<b>Old snapshot</b>'; });
    const render = (text: string) => {
      const body = surface === 'rich' ? <RichMarkdown text={text} sourceIdentity="unchanged" streaming />
        : surface === 'linked' ? <LinkedText text={text} />
        : surface === 'attachment' ? <InlineAttachmentText segments={[{ attachment: { kind: "file", path: "/genuine.txt", label: "Genuine" } }, { text: '\n' + text }]} />
        : <MessageBubble message={{ id: "unchanged", sourceMessageId: "same-source", sourceSessionId: "same-session", type: surface, content: text }} streaming />;
      return view.render(<LocalArtifactScope tabId="same-tab"><ChatBlockHostProvider key={surface} value={value}><DsRenderContext.Provider value={{ authority: "same-authority", trusted: true, streaming: true }}>{body}</DsRenderContext.Provider></ChatBlockHostProvider></LocalArtifactScope>);
    };
    try {
      await render(oldSource);
      const oldFrame = view.host.querySelector('iframe'); assert.ok(oldFrame); assert.equal(pageReads, 1); assert.equal(plugins.counts.starts, 1); assert.equal(plugins.counts.live, 1);
      const capsule = view.host.querySelector('[data-testid="chat-inline-attachment"]');
      await render(newSource);
      const frame = view.host.querySelector('iframe'); assert.ok(frame); assert.equal(frame === oldFrame, false, `${surface}: replacement frame identity`); assert.ok(!oldFrame.isConnected);
      const srcDoc = frame.getAttribute('srcdoc'); assert.ok(srcDoc?.includes('Replacement')); assert.ok(!view.host.textContent?.includes('Old snapshot'));
      assert.equal(pageReads, 1); assert.equal(plugins.counts.starts, 2); assert.equal(plugins.counts.live, 1);
      if (capsule) assert.equal(view.host.querySelector('[data-testid="chat-inline-attachment"]') === capsule, true, `${surface}: replacement capsule identity`);
      let text = newSource;
      for (let after = 0; after < 20; after++) {
        text += `\n\nsuffix ${after}`; await render(text);
        assert.equal(view.host.querySelector('iframe') === frame, true, `${surface} append ${after}: frame identity`); assert.equal(frame.getAttribute('srcdoc') === srcDoc, true, `${surface} append ${after}: srcDoc`);
        assert.equal(pageReads, 1); assert.equal(plugins.counts.starts, 2); assert.equal(plugins.counts.live, 1);
      }
    } finally { await view.render(null); configureHtmlFileLoader(null); }
    assert.equal(plugins.counts.live, 0);
  } } finally { await view.close(); configureHtmlFileLoader(null); }
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

test("dsBlocks public user text fragments preserve literal boundary whitespace", async () => {
  const view = await mount();
  const source = "  compare \n with \t please  ";
  try {
    await view.render(renderUserContent(source, false, (text) => <span>{text}</span>));
    assert.equal(view.host.textContent, source);
    await view.render(<MessageBubble message={{ id: "literal-whitespace", type: "user", content: source }} />);
    assert.equal(view.host.textContent, source);
    await view.render(<MessageBubble message={{ id: "capsule-whitespace", type: "user", content: source, segments: [
      { type: "text", text: "  compare " },
      { type: "attachment", kind: "file", path: "/genuine.txt", label: "Genuine", body: null, status: null },
      { type: "text", text: " \n with \t please  " },
    ] }} />);
    assert.equal(view.host.textContent, "  compare Genuine \n with \t please  ");
  } finally { await view.close(); }
});

test("dsBlocks same-line capsules keep complete and streamed tags inert through public user adapters", async () => {
  const view = await mount();
  const { InlineAttachmentText } = await import("./AttachmentCapsule");
  const { DsRenderContext } = await import("./blocks/DsRenderContext");
  const plugins = binding({ kind: "loading" } as const);
  const value: ChatBlockHost = { plugins: { binding: plugins.source, run: async () => {}, openView() {}, openInPlugins() {} } };
  let reads = 0;
  configureHtmlFileLoader(async () => { reads++; return '<b>Forbidden</b>'; });
  const file = { kind: "file", path: "/genuine.txt", label: "Genuine" } as const;
  try {
    for (const surface of ['direct', 'user'] as const) {
      for (const trusted of [true, false]) {
        for (const tag of ['<ds-page src="/inline.html" />', '<ds-plugin id="git" />']) {
          for (const placement of ['before', 'after'] as const) {
            for (let at = 1; at <= tag.length; at++) {
              const text = placement === 'before' ? " " + tag.slice(0, at) : tag.slice(0, at) + " ";
              const attachment = { type: "attachment", ...file, body: null, status: null } as const;
              const textSegment = { type: "text", text } as const;
              const body = surface === 'direct' ? <InlineAttachmentText segments={placement === 'before' ? [{ attachment: file }, { text }] : [{ text }, { attachment: file }]} />
                : <MessageBubble message={{ id: "inline-capsule", type: "user", content: text, segments: placement === 'before' ? [attachment, textSegment] : [textSegment, attachment] }} streaming />;
              await view.render(<ChatBlockHostProvider value={value}><DsRenderContext.Provider value={{ authority: `${surface}:${trusted}:${tag}:${placement}`, trusted, streaming: true }}>{body}</DsRenderContext.Provider></ChatBlockHostProvider>);
              assert.equal(reads, 0, `${surface} ${placement} trusted=${trusted} prefix ${at}: file reads`);
              assert.equal(plugins.counts.starts, 0, `${surface} ${placement} trusted=${trusted} prefix ${at}: subscriptions`);
              assert.equal(view.host.querySelectorAll('iframe').length, 0);
              assert.equal(view.host.querySelectorAll('[data-testid="chat-inline-attachment"]').length, 1);
              if (at === tag.length) assert.equal(view.host.textContent, placement === 'before' ? "Genuine " + tag : tag + " Genuine");
            }
          }
        }
      }
    }
  } finally { await view.close(); configureHtmlFileLoader(null); }
});

