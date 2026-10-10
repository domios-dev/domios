import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { NodeFiles } from "./NodeFiles";
import { filePreviewKind, revealFileLabel } from "./nodeFilesModel";
import { sentinelIdentity } from "./fixtures.testdata";
import type { NodeFilesAdapter } from "./useNodeFiles";

async function main() {
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const files = [
    { name: "run.py", path: "C:/workspace/scripts/run.py", short_path: "scripts/run.py", exists: true },
    { name: "input.json", path: "C:/workspace/data/input.json", short_path: "data/input.json", exists: true },
    { name: "missing.txt", path: "C:/workspace/missing.txt", short_path: "missing.txt", exists: false },
  ];
  const revealed: string[] = [];
  const opened: string[] = [];
  const adapter: NodeFilesAdapter = {
    read: async (identity, bucket, handler) => {
      assert.deepEqual(identity, sentinelIdentity);
      assert.equal(bucket, "ready");
      assert.equal(handler, "script");
      return files;
    },
    reveal: async (path) => { revealed.push(path); },
    open: async (path) => { opened.push(path); },
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<NodeFiles identity={sentinelIdentity} bucket="ready" handler="script" adapter={adapter} />));
    const rows = host.querySelectorAll("ul > li");
    assert.equal(rows.length, 3);
    const reveal = rows[0].querySelectorAll<HTMLButtonElement>("button")[1];
    await act(async () => reveal.click());
    assert.deepEqual(revealed, [files[0].path]);
    await act(async () => rows[0].querySelector<HTMLButtonElement>("button")!.click());
    assert.deepEqual(opened, [files[0].path]);
    assert.ok(rows[2].textContent?.includes("not found"));
    assert.ok([...rows[2].querySelectorAll<HTMLButtonElement>("button")].every((button) => button.disabled));
    assert.equal(host.querySelector("input, textarea, [contenteditable=true]"), null);
    assert.equal(revealFileLabel("MacIntel"), "Show in Finder");
    assert.equal(revealFileLabel("Win32"), "Show in Explorer");
    assert.equal(revealFileLabel("Linux x86_64"), "Show in File Manager");
    assert.equal(filePreviewKind("/workspace/input.json"), "document");
    assert.equal(filePreviewKind("/workspace/run.py"), null);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log("Node files use exact definition paths and OS opener actions");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
