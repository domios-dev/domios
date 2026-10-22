import assert from "node:assert/strict";
import { test } from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LocalArtifactBlock } from "./LocalArtifactBlock";
import { configureLocalArtifactLoader, configureLocalArtifactOpener, type LocalPreviewFile } from "./localArtifactLoader";

const window = new Window();
for (const key of ["window", "document", "HTMLElement", "Element", "Node"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const file: LocalPreviewFile = { data: "data:image/png;base64,AA==", media_type: "image/png" };

test("local gallery caps reads and displays resolved slots while other reads remain pending", async () => {
  const pending = new Map<string, (file: LocalPreviewFile) => void>();
  const calls: string[] = [];
  configureLocalArtifactLoader(path => { calls.push(path); return new Promise(resolve => pending.set(path, resolve)); });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const paths = Array.from({ length: 100 }, (_, index) => `/image-${index}.png`);
  try {
    await act(async () => root.render(<LocalArtifactBlock directive={{ kind: "image", path: paths[0], paths }} renderMarkdown={() => null} />));
    assert.deepEqual(calls, paths.slice(0, 2));
    assert.equal(host.querySelectorAll('[aria-label^="Fullscreen image-"]').length, 32);
    assert(host.querySelector('[role="alert"]'));
    await act(async () => pending.get(paths[0])!(file));
    assert.equal(host.querySelectorAll("img").length, 1);
    assert.deepEqual(calls, paths.slice(0, 3));
    assert(host.querySelector('[aria-label="Loading image-1.png"]'));
  } finally {
    await act(async () => root.unmount());
    pending.forEach(resolve => resolve(file)); host.remove(); configureLocalArtifactLoader(null);
  }
});

test("single-image preview retains its region, filename/path header, reveal and error renderer", async () => {
  const path = "/work/single image.png";
  let revealed = "";
  configureLocalArtifactLoader(async () => file);
  configureLocalArtifactOpener(async value => { revealed = value; });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    const props = { directive: { kind: "image" as const, path }, renderMarkdown: () => null, renderError: (error: string) => <strong role="alert" data-reason={error} /> };
    await act(async () => root.render(<LocalArtifactBlock {...props} />));
    assert(host.querySelector('[aria-label="image preview: single image.png"]'));
    assert(host.querySelector('header [title="/work/single image.png"]'));
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Reveal single image.png"]')!.click());
    assert.equal(revealed, path);
    configureLocalArtifactLoader(async () => { throw new Error("opaque refusal reason"); });
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Reload single image.png"]')!.click());
    assert.equal(host.querySelector('[role="alert"]')?.getAttribute("data-reason"), "opaque refusal reason");
    assert(host.querySelector('[aria-label="single image.png not available"]'));
    assert.equal(host.querySelector("img"), null);
  } finally {
    await act(async () => root.unmount()); host.remove(); configureLocalArtifactLoader(null); configureLocalArtifactOpener(null);
  }
});

test("repeated paths retain separate image slots", async () => {
  configureLocalArtifactLoader(async () => file);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<LocalArtifactBlock directive={{ kind: "image", path: "/same.png", paths: ["/same.png", "/same.png"] }} renderMarkdown={() => null} />));
    assert.equal(host.querySelectorAll('img[alt="same.png"]').length, 2);
  } finally { await act(async () => root.unmount()); host.remove(); configureLocalArtifactLoader(null); }
});
