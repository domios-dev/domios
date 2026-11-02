import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { LiveBindingSource } from "./blocks/liveBinding";

export const browser = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLIFrameElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? browser : browser[key], configurable: true });
}
browser.matchMedia = (() => ({ matches: false, media: "", onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => false })) as typeof browser.matchMedia;
export const mermaidCalls: string[] = [];
Object.assign(browser, { mermaid: { initialize() {}, async render(_id: string, source: string) { mermaidCalls.push(source); return { svg: '<svg xmlns="http://www.w3.org/2000/svg"><text>Diagram</text></svg>' }; } } });
export const { MessageBubble } = await import("./MessageBubble");
export const { MessagePartsContext, MessagePartView } = await import("./MessagePartsContext");
export const { RichMarkdown, renderUserContent } = await import("./MarkdownBody");
export const { ChatBlockHostProvider } = await import("./blocks/ChatBlockHost");
export const { LocalArtifactScope } = await import("./LocalArtifactScope");
export const { configureHtmlFileLoader } = await import("./htmlFileLoader");
export const { configureLocalArtifactLoader } = await import("./localArtifactLoader");

export function binding<T>(value: T, event?: (value: string) => void) {
  const counts = { starts: 0, live: 0, refresh: 0 };
  const source: LiveBindingSource<T> = { read: () => value, refresh: (key) => { counts.refresh++; event?.(`refresh:${key}`); }, subscribe: () => { counts.starts++; counts.live++; event?.("subscribe"); return () => { counts.live--; event?.("unsubscribe"); }; } };
  return { source, counts };
}
export async function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  return { host, render: (ui: React.ReactNode) => act(async () => root.render(ui)), transition: (ui: React.ReactNode) => act(async () => React.startTransition(() => root.render(ui))), close: async () => { await act(async () => root.unmount()); host.remove(); } };
}
export const html = '<ds-html><input/><svg><path/></svg><script>const outer="</ds-html>";</script><button>Local action</button></ds-html>';

