import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import type { MessageSegment, NormalizedChatMessage } from "@codeterm/chat-core";
import { formatAttachmentMarker, projectTranscriptParts } from "@codeterm/chat-core";
import { deliveryProjectionSends, type DeliverySend } from "../../../src/components/Terminal/chatDeliveryModel";
import { enrichTimelineItems } from "../../../src/components/Terminal/chatEnrichment";
import { MessageBubble } from "./MessageBubble";
import { LocalArtifactScope } from "./LocalArtifactScope";
import { configureLocalArtifactLoader, type LocalPreviewFile } from "./localArtifactLoader";
import fixtureData from "../../../tests/fixtures/message-envelope.json";

for (const kind of ["tab"] as const) {
  test("queued and echoed routed tab message preserves its card header", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const attrs = { from: kind, label: "Weaver", tab: "8494e5c7", model: "opaque-model", user: "123", peer: "peer" };
    const wire = `<domios from="${kind}" label="Weaver" tab="8494e5c7" model="opaque-model">**Opaque body**<tip>Opaque reply command</tip></domios>`;
    const segments: MessageSegment[] = [kind === "curator"
      ? { type: "unknown_frame", raw: wire, attrs, body: "**Opaque body**<tip>Opaque reply command</tip>" }
      : { type: "frame", kind, from: kind, attrs, body: "**Opaque body**", tip: "Opaque reply command" }];
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let focused = false;
    try {
      for (const state of ["held", "delivered"] as const) {
        const send: DeliverySend = { id: "host", correlationId: "token", content: wire, sentAtMs: 1000,
          status: { tab_id: "target", send_id: "host", correlationId: "token", state, ledger_id: "ledger", sequence: 1,
            retained_from: 1, error: null, system_send: { content: wire, sent_at_ms: 1000, segments, segments_version: 2 } } };
        const sends = deliveryProjectionSends([send]);
        const messages = state === "held" ? [{ id: "host", type: "user" as const, content: wire, segments: sends[0].segments }]
          : projectTranscriptParts([{ id: "echo", type: "user", content: wire, correlationId: "token", segments: [{ type: "text", text: wire }] }], sends);
        const [entry] = enrichTimelineItems(messages.map(message => ({ kind: "message" as const, message })), new WeakMap());
        await act(async () => root.render(<MessageBubble key={state} message={entry.msg} origin={entry.origin} onOriginClick={() => { focused = true; }} />));
        const card = host.querySelector<HTMLElement>('[data-testid="chat-inbound-message"]');
        expect(card?.textContent).toContain("Received from Weaver");
        expect(card?.textContent).toContain("opaque-model");
        expect(host.textContent).not.toContain("Reply details");
        expect(host.textContent).not.toContain("Open sender tab");
        expect(host.textContent).not.toContain(`${kind} · Weaver · opaque-model`);
        expect(host.textContent).not.toContain("<domios");
        expect(host.textContent).not.toContain("<tip>");
        expect(host.textContent).not.toContain("Opaque reply command");
        const opener = host.querySelector<HTMLButtonElement>('button[aria-label="Expand tool details"]');
        if (opener) await act(async () => opener.click());
        const notice = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
        if (!host.querySelector("strong") && notice) await act(async () => notice.click());
        expect(host.querySelector("strong")?.textContent).toBe("Opaque body");
        focused = false;
        await act(async () => card?.querySelector<HTMLButtonElement>('button[title="Show Tab"]')?.click());
        expect(focused).toBe(true);
      }
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

test("merged user and routed turn renders a user bubble and a named routed card", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fixtures = fixtureData as Array<{ name: string; raw: string; segments: MessageSegment[] }>;
  const fixture = fixtures.find((entry) => entry.name === "owner merged screenshot")!;
  const rows = enrichTimelineItems([{ kind: "message", message: { id: "merged", type: "user", content: fixture.raw, segments: fixture.segments } }], new WeakMap());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>{rows.map((row) => <MessageBubble key={row.msg.id} message={row.msg} origin={row.origin} />)}</>));
    const user = host.querySelectorAll('[role="group"][aria-label="User message"]');
    const routed = host.querySelectorAll('[role="group"][aria-label="Received from Probe"]');
    expect(user).toHaveLength(1);
    expect(routed).toHaveLength(1);
    expect(user[0].textContent?.trim()).toBe("какой щас план?");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("restored user caption starts at the top of its bubble after a restart notice", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const attrs = { from: "system", context: "turn-resume" };
  const notice = "Continue the interrupted work.";
  const firstText = "\n".repeat(8) + "Check the shadows ";
  const path = String.raw`C:\Temp\codeterm-chat\0123456789abcdef0123456789abcdef-image.png`;
  const lastText = " and adjust them.";
  const segments: MessageSegment[] = [
    { type: "frame", kind: "system", from: "system", attrs, body: notice, tip: null },
    { type: "text", text: firstText },
    { type: "attachment", kind: "image", path, label: "image1", body: null, status: null },
    { type: "text", text: lastText },
  ];
  const content = `<domios from="system" context="turn-resume">${notice}</domios>${firstText}${formatAttachmentMarker(path, "image", "image1")}${lastText}`;
  const restored: NormalizedChatMessage = JSON.parse(JSON.stringify({ id: "restored", type: "user", content, segments }));
  const rows = enrichTimelineItems([{ kind: "message", message: restored }], new WeakMap());
  let reject!: (reason: Error) => void;
  const promise = new Promise<LocalPreviewFile>((_, onReject) => { reject = onReject; });
  const readImage = vi.fn(() => promise);
  configureLocalArtifactLoader(readImage);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<LocalArtifactScope tabId="restored-tab">
      {rows.map((row) => <MessageBubble key={row.msg.id} message={row.msg} origin={row.origin} />)}
    </LocalArtifactScope>));
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    const bubble = host.querySelector('[role="group"][aria-label="User message"]')!;
    const chip = bubble.querySelector<HTMLButtonElement>('button[aria-label="Open image image.png"]')!;
    expect(chip).not.toBeNull();
    expect(readImage).toHaveBeenCalledWith(path, "image", "restored-tab");
    expect(bubble.textContent).toBe("Check the shadows image.png and adjust them.");
    await act(async () => chip.focus());
    const preview = document.querySelector('[role="region"][aria-label="Preview image.png"]');
    expect(preview).not.toBeNull();
    expect(bubble.contains(preview)).toBe(false);
    await act(async () => reject(new Error("Image unavailable")));
    expect(document.querySelector('[role="region"][aria-label="Preview image.png"]')).toBeNull();
    expect(bubble.contains(chip)).toBe(true);
    expect(bubble.textContent).toBe("Check the shadows image.png and adjust them.");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    configureLocalArtifactLoader(null);
  }
});

test("routed card renders Rust attachment segments beside its body", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fixture = (fixtureData as Array<{ name: string; raw: string; segments: MessageSegment[] }>).find((entry) => entry.name === "routed attachment body")!;
  const rows = enrichTimelineItems([{ kind: "message", message: { id: "attachment", type: "user", content: fixture.raw, segments: fixture.segments } }], new WeakMap());
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MessageBubble message={rows[0].msg} origin={rows[0].origin} />));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Expand tool details"]')?.click());
    expect(host.querySelector('button[aria-label="Open image image.png"]')).not.toBeNull();
    expect(host.textContent).toContain("note");
    expect(host.textContent).toContain("done");
  } finally {
    await act(async () => root.unmount());
  }
});

test("agent sender fixture renders its model and message-specific controls", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fixture = (fixtureData as Array<{ name: string; raw: string; segments: MessageSegment[] }>).find((entry) => entry.name === "agent sender with model")!;
  const [entry] = enrichTimelineItems([{ kind: "message", message: { id: "agent-sender", type: "user", content: fixture.raw, segments: fixture.segments } }], new WeakMap());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MessageBubble message={entry.msg} origin={entry.origin} onOriginClick={() => {}} />));
    expect(host.querySelector('[role="group"][aria-label="Received from AGENT"]')?.textContent).toContain("gpt-6.1-sol");
    expect(host.querySelector('button[aria-label="Show tab AGENT"]')).not.toBeNull();
    const collapseControls = [...host.querySelectorAll<HTMLButtonElement>('button[aria-label="Collapse message"]')];
    expect(collapseControls).toHaveLength(2);
    expect(collapseControls.filter((button) => button.tabIndex >= 0)).toHaveLength(1);
    await act(async () => collapseControls.find((button) => button.tabIndex >= 0)?.click());
    expect(host.querySelector('button[aria-label="Expand message"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

for (const structured of [false, true]) {
  test(`relayed inline capsules keep one continuous paragraph (${structured ? "segments" : "markers"})`, async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const content = 'Review **these files**: <domios type="attachment" kind="file" path="/project/config.json" /> alongside <domios type="attachment" kind="file" path="/project/typography.md" /> before sending.';
    const segments: MessageSegment[] | undefined = structured ? [
      { type: "text", text: "Review **these files**: " },
      { type: "attachment", kind: "file", path: "/project/config.json", label: null, body: null, status: null },
      { type: "text", text: " alongside " },
      { type: "attachment", kind: "file", path: "/project/typography.md", label: null, body: null, status: null },
      { type: "text", text: " before sending." },
    ] : undefined;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<MessageBubble message={{ id: "inline-relayed", type: "user", content, segments }} origin={{ kind: "tab", title: "Elgar", tabIdShort: "elgar" }} />));
      const card = host.querySelector('[role="group"][aria-label="Received from Elgar"]')!;
      expect(card).not.toBeNull();
      const paragraph = card.querySelector('[data-testid="chat-inline-attachment-text"]')!;
      expect(paragraph).not.toBeNull();
      expect(paragraph.querySelectorAll("button")).toHaveLength(2);
      expect(paragraph.querySelector("strong")?.textContent).toBe("these files");
      expect(paragraph.textContent).toBe("Review these files: config.json alongside typography.md before sending.");
      expect(paragraph.querySelectorAll("p, div, pre")).toHaveLength(0);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

for (const pasted of [false, true]) {
  test(`bounds a long ${pasted ? "pasted envelope" : "plain user message"} with an inline reader under the bubble`, async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const body = "Opaque user paragraph\n".repeat(400) + "Final user sentinel";
    const raw = `<pasted_content id="opaque">\n${body}\n</pasted_content id="opaque">\n\npasted by Domios`;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const render = (suffix = "") => {
      const rows = enrichTimelineItems([{ kind: "message", message: {
        id: "long-user", type: "user", content: pasted ? raw + suffix : body + suffix,
        segments: pasted ? [{ type: "paste", id: "opaque", body: body + suffix }] : undefined,
      } }], new WeakMap());
      return <MessageBubble message={rows[0].msg} origin={rows[0].origin} />;
    };
    try {
      await act(async () => root.render(render()));
      const bubble = host.querySelector<HTMLElement>('[role="group"][aria-label="User message"]')!;
      Object.defineProperties(bubble, { scrollHeight: { value: 5910 }, clientHeight: { value: 320 } });
      await act(async () => root.render(render("\n")));
      const opener = host.querySelector<HTMLButtonElement>('button[aria-label="Open full screen: Message content"]')!;
      expect(opener).toBeTruthy();
      expect(bubble.contains(opener)).toBe(false);
      const disclosure = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]')!;
      expect(disclosure).toBeTruthy();
      await act(async () => disclosure.click());
      expect(disclosure.getAttribute("aria-expanded")).toBe("true");
      await act(async () => disclosure.click());
      expect(disclosure.getAttribute("aria-expanded")).toBe("false");
      await act(async () => opener.click());
      expect(document.querySelector('[role="dialog"] [aria-label="Reader content"]')?.textContent).toContain("Final user sentinel");
      await act(async () => document.querySelector<HTMLButtonElement>('button[aria-label="Close fullscreen viewer"]')!.click());
      expect(document.querySelector('[role="dialog"]')).toBeNull();
      expect(document.activeElement).toBe(opener);
      expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}
