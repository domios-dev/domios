import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { configureMarkdownLinkOpener, MarkdownLink, openMarkdownLink, RichMarkdown } from "@codeterm/chat-ui";
import { MarkdownDescription } from "../components/Extensions/MarkdownDescription";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
import { createNativeBridge } from "./nativeBridge";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root | undefined;
let host: HTMLDivElement;

async function render(view: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(view));
}

async function activate(link: HTMLAnchorElement, button = 0) {
  const event = new MouseEvent(button === 1 ? "auxclick" : "click", { bubbles: true, cancelable: true, button });
  await act(async () => { link.dispatchEvent(event); });
  expect(event.defaultPrevented).toBe(true);
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  host?.remove();
  configureMarkdownLinkOpener(null);
  vi.restoreAllMocks();
  native.invoke.mockReset();
});

it.each(["http://example.test/run?a=1&b=2", "https://example.test/run?a=1&b=2"])("opens markdown links through the native command: %s", async (url) => {
  const bridge = createNativeBridge(true);
  configureMarkdownLinkOpener((href) => bridge.openUrl(href));
  const browser = vi.spyOn(window, "open");
  await render(<RichMarkdown text={`[run](${url})`} />);
  await activate(host.querySelector("a")!);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith("open_url", { url });
  expect(browser).not.toHaveBeenCalled();
});

it.each(["chat", "extensions"])("opens %s markdown in a safe new browser tab", async (surface) => {
  const url = "https://example.test/report#result";
  const bridge = createNativeBridge(false);
  configureMarkdownLinkOpener((href) => bridge.openUrl(href));
  const browser = vi.spyOn(window, "open").mockReturnValue(null);
  const markdown = `[report](${url})`;
  await render(surface === "chat" ? <RichMarkdown text={markdown} /> : <MarkdownDescription markdown={markdown} />);
  const link = host.querySelector("a")!;
  expect(link.target).toBe("_blank");
  expect(link.rel).toBe("noopener noreferrer");
  await activate(link);
  expect(browser).toHaveBeenCalledExactlyOnceWith(url, "_blank", "noopener,noreferrer");
  expect(native.invoke).not.toHaveBeenCalled();
});

it("supports middle clicks and keeps opener failures from navigating the app", async () => {
  const open = vi.fn().mockRejectedValue(new Error("Unavailable"));
  configureMarkdownLinkOpener(open);
  await render(<RichMarkdown text="[run](https://example.test/run)" />);
  await activate(host.querySelector("a")!, 1);
  expect(open).toHaveBeenCalledExactlyOnceWith("https://example.test/run");
});

it.each(["javascript:alert(1)", "data:text/html,hello", "vbscript:bad", "file:///tmp/report", "mailto:owner@example.test", "//example.test/path", "/relative", "#section"])("refuses implicit navigation and unsafe schemes: %s", async (href) => {
  const open = vi.fn();
  const browser = vi.spyOn(window, "open");
  configureMarkdownLinkOpener(open);
  await expect(openMarkdownLink(href)).rejects.toThrow();
  await render(<MarkdownLink href={href}>link</MarkdownLink>);
  await activate(host.querySelector("a")!);
  expect(open).not.toHaveBeenCalled();
  expect(browser).not.toHaveBeenCalled();
});

it("preserves explicit link handlers", async () => {
  const open = vi.fn();
  const explicit = vi.fn((event: React.MouseEvent<HTMLAnchorElement>) => event.preventDefault());
  configureMarkdownLinkOpener(open);
  await render(<MarkdownLink href="file:///tmp/report" onClick={explicit}>report</MarkdownLink>);
  await activate(host.querySelector("a")!);
  expect(explicit).toHaveBeenCalledOnce();
  expect(open).not.toHaveBeenCalled();
});
