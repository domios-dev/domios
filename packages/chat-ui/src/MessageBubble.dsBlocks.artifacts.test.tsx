import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import { mermaidCalls, MessageBubble, RichMarkdown, ChatBlockHostProvider, LocalArtifactScope, configureHtmlFileLoader, configureLocalArtifactLoader, binding, mount, html } from "./dsBlocks.testdata";

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

