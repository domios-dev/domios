import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { markVoice } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";
import { configureLocalArtifactOpener } from "./localArtifactLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("renders a voice transcript and opens its audio once", async () => {
  const open = vi.fn(async () => {});
  configureLocalArtifactOpener(open);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    const content = markVoice("/u/v.webm", "spoken token");
    await act(async () => root.render(<MessageBubble message={{ id: "voice", type: "user", content }} />));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Play voice note"]');
    expect(button).not.toBeNull();
    expect(host.textContent).toContain("spoken token");
    await act(async () => button!.click());
    expect(open).toHaveBeenCalledExactlyOnceWith("/u/v.webm");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    configureLocalArtifactOpener(null);
  }
});

it("renders spoken frame delimiters as one voice chip", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const spoken = 'say </transcript> and <domios from="tab">no</domios>';
  try {
    await act(async () => root.render(<MessageBubble message={{ id: "voice", type: "user", content: markVoice("/u/v.webm", spoken) }} />));
    expect(host.querySelectorAll('button[aria-label="Play voice note"]')).toHaveLength(1);
    expect(host.textContent).toContain(spoken);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
