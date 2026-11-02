import assert from "node:assert/strict";
import test from "node:test";
import type { ChatBlockHost } from "./blocks/ChatBlockHost";
import { MessageBubble, RichMarkdown, renderUserContent, ChatBlockHostProvider, LocalArtifactScope, configureHtmlFileLoader, binding, mount, html } from "./dsBlocks.testdata";

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
            for (const at of [1, 4, tag.length - 2, tag.length - 1, tag.length]) {
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

